using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Security.Cryptography;

// Prove the malformed return tail is unreachable. Never change the source PE.
public static class ManagedThrowStubs {
    static string Hex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();
    static string Hash(byte[] bytes) => Hex(SHA256.HashData(bytes));

    static bool TypeIs(MetadataReader reader, EntityHandle handle, string space, string name) {
        if (handle.Kind == HandleKind.TypeDefinition) {
            var type = reader.GetTypeDefinition((TypeDefinitionHandle)handle);
            return type.GetDeclaringType().IsNil && reader.GetString(type.Namespace) == space
                && reader.GetString(type.Name) == name;
        }
        if (handle.Kind == HandleKind.TypeReference) {
            var type = reader.GetTypeReference((TypeReferenceHandle)handle);
            return type.ResolutionScope.Kind != HandleKind.TypeReference
                && reader.GetString(type.Namespace) == space && reader.GetString(type.Name) == name;
        }
        return false;
    }

    static bool GenericClassReturn(MetadataReader reader, MethodDefinition method) {
        var signature = reader.GetBlobReader(method.Signature);
        var header = signature.ReadSignatureHeader();
        if (header.Kind != SignatureKind.Method || header.IsGeneric
                || header.CallingConvention != SignatureCallingConvention.Default) return false;
        signature.ReadCompressedInteger(); // Parameter count precedes the return type.
        if (signature.ReadByte() != 0x15 || signature.ReadByte() != 0x12) return false;
        var type = signature.ReadTypeHandle();
        if (type.Kind == HandleKind.TypeDefinition) reader.GetTypeDefinition((TypeDefinitionHandle)type);
        else if (type.Kind == HandleKind.TypeReference) reader.GetTypeReference((TypeReferenceHandle)type);
        else return false;
        return signature.ReadCompressedInteger() > 0;
    }

    static bool Constructor(MetadataReader reader, EntityHandle handle, out byte[] signature) {
        signature = null;
        EntityHandle parent;
        string name;
        if (handle.Kind == HandleKind.MethodDefinition) {
            var method = reader.GetMethodDefinition((MethodDefinitionHandle)handle);
            if ((method.Attributes & MethodAttributes.Static) != 0) return false;
            parent = method.GetDeclaringType(); name = reader.GetString(method.Name);
            signature = reader.GetBlobBytes(method.Signature);
        } else if (handle.Kind == HandleKind.MemberReference) {
            var member = reader.GetMemberReference((MemberReferenceHandle)handle);
            parent = member.Parent; name = reader.GetString(member.Name);
            signature = reader.GetBlobBytes(member.Signature);
        } else return false;
        return name == ".ctor" && TypeIs(reader, parent, "System", "PlatformNotSupportedException")
            && Hex(signature) == "200001";
    }

    public static Dictionary<string, object> Prove(MetadataReader reader,
            Func<int, MethodBodyBlock> readBody, MethodDefinitionHandle handle) {
        try {
            var method = reader.GetMethodDefinition(handle);
            if (method.RelativeVirtualAddress == 0) return null;
            var caller = readBody(method.RelativeVirtualAddress);
            var il = caller.GetILBytes();
            if (il.Length != 7 || il[0] != 0x28 || il[5] != 0x16 || il[6] != 0x2a
                    || !caller.LocalSignature.IsNil || caller.ExceptionRegions.Length != 0
                    || !GenericClassReturn(reader, method)) return null;
            int calleeToken = BitConverter.ToInt32(il, 1);
            var calleeHandle = MetadataTokens.EntityHandle(calleeToken);
            if (calleeHandle.Kind != HandleKind.MethodDefinition) return null;
            var callee = reader.GetMethodDefinition((MethodDefinitionHandle)calleeHandle);
            var calleeSignature = reader.GetBlobBytes(callee.Signature);
            if ((callee.Attributes & MethodAttributes.Static) == 0
                    || reader.GetString(callee.Name) != "ThrowNotSupportedException"
                    || !TypeIs(reader, callee.GetDeclaringType(), "Unity", "ThrowStub")
                    || Hex(calleeSignature) != "000001" || callee.RelativeVirtualAddress == 0) return null;
            var body = readBody(callee.RelativeVirtualAddress);
            var calleeIl = body.GetILBytes();
            if (calleeIl.Length != 6 || calleeIl[0] != 0x73 || calleeIl[5] != 0x7a
                    || !body.LocalSignature.IsNil || body.ExceptionRegions.Length != 0) return null;
            int constructorToken = BitConverter.ToInt32(calleeIl, 1);
            if (!Constructor(reader, MetadataTokens.EntityHandle(constructorToken), out var constructorSignature)) return null;
            return new Dictionary<string, object> {
                {"kind", "verified_nonreturning_stub"}, {"method_token", MetadataTokens.GetToken(handle)},
                {"caller_il_hex", Hex(il)}, {"caller_il_sha256", Hash(il)},
                {"callee_token", calleeToken}, {"callee_signature", Hex(calleeSignature)},
                {"callee_il_hex", Hex(calleeIl)}, {"callee_il_sha256", Hash(calleeIl)},
                {"exception_constructor_token", constructorToken},
                {"exception_constructor_type", "System.PlatformNotSupportedException"},
                {"exception_constructor_signature", Hex(constructorSignature)}
            };
        } catch (BadImageFormatException) {
            return null;
        } catch (ArgumentException) {
            return null;
        } catch (InvalidOperationException) {
            return null;
        }
    }
}
