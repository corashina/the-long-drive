using System;
using System.IO;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using System.Security.Cryptography;
public static class ManagedAssetIndex {
    static Dictionary<ushort, OpCode> opcodes = new Dictionary<ushort, OpCode>();
    static ManagedAssetIndex() {
        foreach (var field in typeof(OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static)) {
            if (field.FieldType == typeof(OpCode)) {
                var op = (OpCode)field.GetValue(null); opcodes[(ushort)op.Value] = op;
            }
        }
    }
    static string TypeName(MetadataReader reader, EntityHandle handle) {
        if (handle.Kind == HandleKind.TypeDefinition) {
            var t = reader.GetTypeDefinition((TypeDefinitionHandle)handle);
            var parent = t.GetDeclaringType();
            if (!parent.IsNil) return TypeName(reader, parent) + "+" + reader.GetString(t.Name);
            var ns = reader.GetString(t.Namespace);
            return (ns.Length > 0 ? ns + "." : "") + reader.GetString(t.Name);
        }
        if (handle.Kind == HandleKind.TypeReference) {
            var t = reader.GetTypeReference((TypeReferenceHandle)handle);
            if (t.ResolutionScope.Kind == HandleKind.TypeReference)
                return TypeName(reader, t.ResolutionScope) + "+" + reader.GetString(t.Name);
            var ns = reader.GetString(t.Namespace);
            return (ns.Length > 0 ? ns + "." : "") + reader.GetString(t.Name);
        }
        return "token:" + MetadataTokens.GetToken(handle).ToString("x8");
    }
    static Dictionary<string,object> Target(MetadataReader reader, EntityHandle handle) {
        if (handle.Kind == HandleKind.MethodSpecification)
            return Target(reader, reader.GetMethodSpecification((MethodSpecificationHandle)handle).Method);
        if (handle.Kind == HandleKind.MemberReference) {
            var member = reader.GetMemberReference((MemberReferenceHandle)handle);
            return new Dictionary<string,object>{{"type",TypeName(reader,member.Parent)},
                {"method",reader.GetString(member.Name)}, {"token",MetadataTokens.GetToken(handle)},
                {"signature",Convert.ToHexString(reader.GetBlobBytes(member.Signature)).ToLowerInvariant()}};
        }
        if (handle.Kind == HandleKind.MethodDefinition) {
            var method = reader.GetMethodDefinition((MethodDefinitionHandle)handle);
            return new Dictionary<string,object>{{"type",TypeName(reader,method.GetDeclaringType())},
                {"method",reader.GetString(method.Name)}, {"token",MetadataTokens.GetToken(handle)},
                {"signature",Convert.ToHexString(reader.GetBlobBytes(method.Signature)).ToLowerInvariant()}};
        }
        throw new InvalidDataException("Invalid call token " + MetadataTokens.GetToken(handle).ToString("x8"));
    }
    static bool AssetCall(Dictionary<string,object> target) {
        string type = (string)target["type"], method = (string)target["method"];
        if (!type.StartsWith("UnityEngine.")) return false;
        string[] assets = {"UnityEngine.Object", "UnityEngine.Resources", "UnityEngine.AssetBundle",
            "UnityEngine.Mesh", "UnityEngine.Material", "UnityEngine.Texture", "UnityEngine.Texture2D",
            "UnityEngine.RenderTexture", "UnityEngine.Cubemap", "UnityEngine.Sprite", "UnityEngine.Shader",
            "UnityEngine.ComputeShader", "UnityEngine.AudioClip", "UnityEngine.AnimationClip",
            "UnityEngine.RuntimeAnimatorController", "UnityEngine.AnimatorOverrideController",
            "UnityEngine.ScriptableObject", "UnityEngine.MaterialPropertyBlock", "UnityEngine.ImageConversion",
            "UnityEngine.ComputeBuffer", "UnityEngine.GraphicsBuffer",
            "UnityEngine.Networking.DownloadHandlerTexture", "UnityEngine.Networking.DownloadHandlerAudioClip"};
        foreach (string asset in assets) if (type == asset) return true;
        return method == "set_bones" || method == "set_rootBone" || method == "set_sharedMesh" ||
            method == "set_runtimeAnimatorController" || method == "set_targetTexture" ||
            method == "set_material" || method == "set_sharedMaterial" || method == "set_materials" ||
            method == "set_sharedMaterials" || method == "set_texture" || method == "set_mainTexture" ||
            method == "set_mesh" || method == "set_clip" || method == "SetPropertyBlock";
    }
    public static Dictionary<string,object> Read(string path, string sourceFile) {
        using (var stream = File.OpenRead(path)) using (var pe = new PEReader(stream)) {
            var reader = pe.GetMetadataReader(); var assembly = reader.GetAssemblyDefinition();
            var types = new List<object>(); var methods = new List<object>();
            var calls = new List<object>(); var references = new List<object>(); var errors = new List<object>();
            var proofs = new List<object>();
            foreach (var referenceHandle in reader.AssemblyReferences) {
                var reference = reader.GetAssemblyReference(referenceHandle);
                references.Add(new Dictionary<string,object>{{"name",reader.GetString(reference.Name)},
                    {"version",reference.Version.ToString()}, {"culture",reader.GetString(reference.Culture)},
                    {"public_key_or_token",Convert.ToHexString(reader.GetBlobBytes(reference.PublicKeyOrToken)).ToLowerInvariant()}});
            }
            foreach (var typeHandle in reader.TypeDefinitions) {
                var type = reader.GetTypeDefinition(typeHandle); string typeName = TypeName(reader,typeHandle);
                types.Add(new Dictionary<string,object>{{"assembly",sourceFile}, {"full_name",typeName},
                    {"token",MetadataTokens.GetToken(typeHandle)}, {"flags",(int)type.Attributes},
                    {"base_type",type.BaseType.IsNil ? null : TypeName(reader,type.BaseType)}});
                foreach (var methodHandle in type.GetMethods()) {
                    var method = reader.GetMethodDefinition(methodHandle); string name = reader.GetString(method.Name);
                    int token = MetadataTokens.GetToken(methodHandle);
                    var parameters = new List<object>();
                    foreach (var parameterHandle in method.GetParameters()) {
                        var parameter = reader.GetParameter(parameterHandle);
                        parameters.Add(new Dictionary<string,object>{{"sequence",parameter.SequenceNumber},
                            {"name",reader.GetString(parameter.Name)}, {"flags",(int)parameter.Attributes}});
                    }
                    var methodRow = new Dictionary<string,object>{{"assembly",sourceFile},{"type",typeName},
                        {"name",name},{"token",token},{"rva",method.RelativeVirtualAddress},
                        {"flags",(int)method.Attributes},{"implementation_flags",(int)method.ImplAttributes},
                        {"signature",Convert.ToHexString(reader.GetBlobBytes(method.Signature)).ToLowerInvariant()},
                        {"parameters",parameters},{"il_sha256",null}};
                    methods.Add(methodRow);
                    if (method.RelativeVirtualAddress == 0) continue;
                    try {
                        byte[] il = pe.GetMethodBody(method.RelativeVirtualAddress).GetILBytes();
                        methodRow["il_sha256"] = Convert.ToHexString(SHA256.HashData(il)).ToLowerInvariant();
                        var proof = ManagedThrowStubs.Prove(reader, rva => pe.GetMethodBody(rva), methodHandle);
                        if (proof != null) proofs.Add(proof);
                        var strings = new List<object>();
                        for (int offset=0; offset<il.Length;) {
                            int start=offset; ushort key=il[offset++];
                            if (key==0xfe) key=(ushort)(0xfe00|il[offset++]);
                            OpCode op=opcodes[key]; int operand=offset; int length;
                            switch(op.OperandType) {
                                case OperandType.InlineNone: length=0;break;
                                case OperandType.ShortInlineBrTarget: case OperandType.ShortInlineI: case OperandType.ShortInlineVar: length=1;break;
                                case OperandType.InlineVar:length=2;break;
                                case OperandType.InlineI8:case OperandType.InlineR:length=8;break;
                                case OperandType.InlineSwitch:length=4+4*BitConverter.ToInt32(il,offset);break;
                                default:length=4;break;
                            }
                            if(length<0 || offset+length>il.Length) throw new InvalidDataException("Truncated IL operand");
                            if(op.OperandType==OperandType.InlineString) {
                                int stringToken=BitConverter.ToInt32(il,operand);
                                strings.Add(new Dictionary<string,object>{{"il_offset",start},
                                    {"value",reader.GetUserString(MetadataTokens.UserStringHandle(stringToken & 0xffffff))}});
                            }
                            if(op.OperandType==OperandType.InlineMethod) {
                                var target=Target(reader,MetadataTokens.EntityHandle(BitConverter.ToInt32(il,operand)));
                                if(AssetCall(target)) calls.Add(new Dictionary<string,object>{{"assembly",sourceFile},
                                    {"type",typeName},{"method",name},{"method_token",token},
                                    {"il_offset",start},{"opcode",op.Name},{"target",target},
                                    {"preceding_string_literals",new List<object>(strings)}});
                            }
                            offset+=length;
                        }
                    } catch(Exception error) {
                        errors.Add(new Dictionary<string,object>{{"assembly",sourceFile},{"method_token",token},
                            {"type",typeName},{"method",name},{"error",error.ToString()}});
                    }
                }
            }
            var module=reader.GetModuleDefinition();
            return new Dictionary<string,object>{{"name",reader.GetString(assembly.Name)},
                {"version",assembly.Version.ToString()}, {"culture",reader.GetString(assembly.Culture)},
                {"public_key",Convert.ToHexString(reader.GetBlobBytes(assembly.PublicKey)).ToLowerInvariant()},
                {"mvid",reader.GetGuid(module.Mvid).ToString()}, {"references",references},
                {"types",types},{"methods",methods},{"asset_calls",calls},{"diagnostics",errors},
                {"verified_throw_stubs",proofs}};
        }
    }
}
