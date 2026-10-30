using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Security.Cryptography;
using System.Text;
using System.Text.RegularExpressions;
using ICSharpCode.Decompiler;
using ICSharpCode.Decompiler.CSharp;
using ICSharpCode.Decompiler.CSharp.Syntax;
using ICSharpCode.Decompiler.IL;

// Load game files as metadata, never as executable assemblies.
public static class ManagedMethodCoverage
{
    const string StubWarning = "IL_0007: Expected Obj, but got I4";
    static readonly Regex Diagnostic = new Regex(
        @"(?m)^\s*//.*(?:Error decompiling|Unknown result type|Expected[ :]|Invalid comparison|ILSpy generated|ILSpy could not decompile).*");

    static string[] Diagnostics(string text) => Diagnostic.Matches(text)
        .Select(match => match.Value.Trim()).Distinct().ToArray();

    static Dictionary<int, List<string>> Observe(CSharpDecompiler decompiler, SyntaxTree tree)
    {
        var methods = new Dictionary<int, List<string>>();
        foreach (var node in tree.DescendantsAndSelf)
        foreach (var annotation in node.Annotations)
        {
            if (!(annotation is ILFunction function) || function.Method == null ||
                function.Method.ParentModule != decompiler.TypeSystem.MainModule ||
                function.Method.MetadataToken.Kind != HandleKind.MethodDefinition)
                continue;
            int token = MetadataTokens.GetToken(function.Method.MetadataToken);
            if (!methods.TryGetValue(token, out var warnings))
                methods[token] = warnings = new List<string>();
            warnings.AddRange(function.Warnings);
        }
        return methods;
    }

    static void ResolveThrowStubs(CSharpDecompiler decompiler, SyntaxTree tree,
        Dictionary<int, Dictionary<string, object>> resolutions)
    {
        var module = decompiler.TypeSystem.MainModule.MetadataFile;
        foreach (var node in tree.DescendantsAndSelf.ToArray())
        foreach (var function in node.Annotations.OfType<ILFunction>())
        {
            if (function.Method == null || function.Method.ParentModule != decompiler.TypeSystem.MainModule ||
                function.Method.MetadataToken.Kind != HandleKind.MethodDefinition ||
                function.Warnings.Count == 0 ||
                function.Warnings.Any(warning => warning != StubWarning)) continue;
            int token = MetadataTokens.GetToken(function.Method.MetadataToken);
            if (resolutions.ContainsKey(token)) continue;
            var proof = ManagedThrowStubs.Prove(module.Metadata, module.GetMethodBody,
                (MethodDefinitionHandle)function.Method.MetadataToken);
            if (proof == null) continue;
            var returns = node.DescendantsAndSelf.OfType<ReturnStatement>().ToArray();
            if (returns.Length != 1 || !(returns[0].Expression is CastExpression cast) ||
                !(cast.Expression is PrimitiveExpression literal) || !(literal.Value is int value) || value != 0)
                continue;

            // The local callee always throws. Normalize only its proven unreachable I4 return tail.
            // Preserve both original IL bodies, hashes, tokens, and diagnostic in the coverage receipt.
            returns[0].Expression = new NullReferenceExpression();
            foreach (var comment in node.DescendantsAndSelf
                .SelectMany(child => child.LeadingTrivia.Concat(child.TrailingTrivia)).OfType<Comment>()
                .Where(comment => comment.Content.Trim() == StubWarning).ToArray()) comment.Remove();
            function.Warnings.RemoveAll(warning => warning == StubWarning);
            resolutions[token] = proof;
        }
    }

    static string TypeName(MetadataReader reader, TypeDefinitionHandle handle)
    {
        var type = reader.GetTypeDefinition(handle);
        var parent = type.GetDeclaringType();
        if (!parent.IsNil) return TypeName(reader, parent) + "+" + reader.GetString(type.Name);
        var ns = reader.GetString(type.Namespace);
        return (ns.Length == 0 ? "" : ns + ".") + reader.GetString(type.Name);
    }

    static string NoBodyReason(MethodDefinition method)
    {
        if ((method.Attributes & MethodAttributes.Abstract) != 0) return "abstract";
        if ((method.Attributes & MethodAttributes.PinvokeImpl) != 0) return "pinvoke";
        if ((method.ImplAttributes & MethodImplAttributes.InternalCall) != 0) return "internal_call";
        if ((method.ImplAttributes & MethodImplAttributes.CodeTypeMask) == MethodImplAttributes.Runtime)
            return "runtime";
        return "missing_il_body";
    }

    public static Dictionary<string, object> Export(string assemblyPath, string outputPath, string stem)
    {
        var settings = new DecompilerSettings();
        var decompiler = new CSharpDecompiler(assemblyPath, settings);
        var module = decompiler.TypeSystem.MainModule.MetadataFile;
        var reader = module.Metadata;
        var diagnostics = new List<string>();
        var observed = new Dictionary<int, List<string>>();
        var resolutions = new Dictionary<int, Dictionary<string, object>>();
        string codeFile = "managed/" + stem + ".cs";
        string supplementFile = "managed/" + stem + ".methods.cs";
        string code;
        try
        {
            var tree = decompiler.DecompileWholeModuleAsSingleFile();
            ResolveThrowStubs(decompiler, tree, resolutions);
            code = tree.ToString(settings.CSharpFormattingOptions);
            observed = Observe(decompiler, tree);
            diagnostics.AddRange(Diagnostics(code));
            diagnostics.AddRange(decompiler.Errors.Select(error => error.ToString()));
        }
        catch (Exception error)
        {
            diagnostics.Add("Error decompiling module: " + error);
            code = "// Whole-module export failed. See the method supplement and coverage receipt.\n";
        }
        File.WriteAllText(Path.Combine(outputPath, stem + ".cs"), code, new UTF8Encoding(false));

        var methods = new List<object>();
        StreamWriter supplement = null;
        try
        {
            foreach (var handle in reader.MethodDefinitions)
            {
                var method = reader.GetMethodDefinition(handle);
                int token = MetadataTokens.GetToken(handle);
                var row = new Dictionary<string, object> {
                    {"token", token}, {"type", TypeName(reader, method.GetDeclaringType())},
                    {"name", reader.GetString(method.Name)}, {"rva", method.RelativeVirtualAddress},
                    {"flags", (int)method.Attributes}, {"implementation_flags", (int)method.ImplAttributes},
                    {"signature", Convert.ToHexString(reader.GetBlobBytes(method.Signature)).ToLowerInvariant()},
                    {"status", "declaration_only"}, {"reason", null}, {"source", null},
                    {"diagnostics", Array.Empty<string>()}, {"il_sha256", null}
                };
                methods.Add(row);
                if (method.RelativeVirtualAddress == 0)
                {
                    row["reason"] = NoBodyReason(method);
                    continue;
                }
                try
                {
                    var il = module.GetMethodBody(method.RelativeVirtualAddress).GetILBytes();
                    row["il_sha256"] = Convert.ToHexString(SHA256.HashData(il)).ToLowerInvariant();
                    string source = codeFile;
                    if (!observed.TryGetValue(token, out var warnings))
                    {
                        // Default C# can hide constructors, accessors, and compiler-generated methods.
                        // Export each hidden MethodDef explicitly and require its ILFunction annotation.
                        var tree = decompiler.Decompile(new[] { (EntityHandle)handle });
                        // ILSpy replaces Errors on its next Decompile call.
                        var errors = decompiler.Errors.Select(error => error.ToString()).ToArray();
                        diagnostics.AddRange(errors);
                        ResolveThrowStubs(decompiler, tree, resolutions);
                        string text = tree.ToString(settings.CSharpFormattingOptions);
                        if (supplement == null)
                            supplement = new StreamWriter(Path.Combine(outputPath, stem + ".methods.cs"),
                                false, new UTF8Encoding(false));
                        supplement.WriteLine("// MethodDef 0x" + token.ToString("x8") + " " + row["type"] + "::" + row["name"]);
                        supplement.Write(text);
                        supplement.WriteLine();
                        source = supplementFile;
                        row["source"] = source;
                        var direct = Observe(decompiler, tree);
                        if (!direct.TryGetValue(token, out warnings))
                            throw new InvalidDataException("No ILFunction for the requested MethodDef token. " +
                                string.Join("\n", errors.Concat(Diagnostics(text))));
                        warnings.AddRange(errors);
                        warnings.AddRange(Diagnostics(text));
                    }
                    var distinct = warnings.Distinct().ToArray();
                    row["source"] = source;
                    row["diagnostics"] = distinct;
                    row["status"] = distinct.Length == 0 ? "decompiled" : "warning";
                    row["reason"] = distinct.Length == 0 ? null : "decompiler_diagnostics";
                    if (distinct.Length == 0 && resolutions.TryGetValue(token, out var proof))
                    {
                        row["resolution"] = proof;
                        row["resolved_diagnostics"] = new[] { StubWarning };
                    }
                }
                catch (Exception error)
                {
                    row["status"] = "failed";
                    row["reason"] = "method_export_failed";
                    row["diagnostics"] = new[] { error.ToString() };
                }
            }
        }
        finally
        {
            supplement?.Dispose();
        }
        return new Dictionary<string, object> {
            {"methods", methods}, {"diagnostics", diagnostics.Distinct().ToArray()},
            {"type_count", reader.TypeDefinitions.Count}, {"method_count", reader.MethodDefinitions.Count},
            {"supplement", supplement == null ? null : supplementFile}
        };
    }
}
