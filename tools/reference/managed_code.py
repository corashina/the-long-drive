"""Checked managed source text and method bodies."""
import json
import re


def managed_code(source, label):
    from .library.reader import checked
    library=source.library
    receipts={record['file']:record for record in library.manifest['managed_analysis']['files']}
    code=checked(library.records_root,receipts['managed/Assembly-CSharp.cs']).decode('utf-8').replace('\r\n','\n')
    decomp=json.loads(checked(library.records_root,receipts['managed/decompilation.json']))
    assembly='TheLongDrive_Data/Managed/Assembly-CSharp.dll'
    if decomp['input']['sha256']!=library.source_files[assembly].sha256:
        raise ValueError(f'{label} managed source provenance differs')
    return code,{'assembly':{'file':assembly,'sha256':decomp['input']['sha256']},'source':receipts['managed/Assembly-CSharp.cs']}

def class_code(code, name):
    start=code.index(f'public class {name} : MonoBehaviour')
    end=code.find('\npublic class ',start+1)
    return code[start:end if end!=-1 else len(code)]

def method(code, name, signature):
    body=class_code(code,name)
    match=re.search(r'(?m)^\s*'+re.escape(signature)+r'\s*\{',body)
    if not match:raise ValueError(f'{name}.{signature} managed method missing')
    depth=1
    for pos in range(match.end(),len(body)):
        if body[pos]=='{':depth+=1
        elif body[pos]=='}':
            depth-=1
            if depth==0:return body[match.end():pos]
    raise ValueError(f'{name}.{signature} managed method is incomplete')

def require(code, required):
    for (name,signature),expressions in required.items():
        body=method(code,name,signature)
        for expression in expressions:
            if expression not in body:raise ValueError(f'{name}.{signature} managed source differs: {expression}')
