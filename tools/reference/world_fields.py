"""Compact immutable runtime fields. Full checked inputs remain in provenance."""
import hashlib
import json
from pathlib import Path


PROVENANCE_ROOT = Path(__file__).resolve().parents[2] / '.cache/world-provenance'


def project_runtime_fields(world):
    original=json.dumps({name:world[name] for name in ('library','source_hashes','records','initialization','receipts')},separators=(',',':')).encode()
    digest=hashlib.sha256(original).hexdigest()
    directory=PROVENANCE_ROOT
    directory.mkdir(parents=True,exist_ok=True)
    path=directory/f'world-records-{digest}.json'
    if path.exists():
        if path.read_bytes()!=original:raise ValueError('Immutable world record provenance differs')
    else:path.write_bytes(original)
    def value(v):
        if v['kind']=='record':v['value']=fields(v['value'])
        elif v['kind']=='array':
            for child in v['value']:value(child)
        return v
    def fields(f):return [dict(name=name,value=value(v)) for name,v in sorted(f.items())]
    for record in world['records']:
        # Component and validation consumers now read the sole link value in fields.
        record['fields']=fields(record['fields'])
        del record['links']
    for component in world['initialization']['components']:
        for field in component['fields']:value(field['value'])
    print(f'Immutable record provenance: {path} SHA256 {digest}',flush=True)
