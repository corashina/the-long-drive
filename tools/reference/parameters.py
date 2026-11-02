"""Recover selected fields and retain their source evidence."""
import hashlib
from .source import key, source_id

REQUIRED_CLASSES = {
    'tankscript','tankcapscript','enginescript','carscript','seatscript','door_rot',
    'massScript','partconditionscript','gumiscript','rimscript','wheelconnectscript',
    'newRandomStuffSpawnScript','tosaveitemscript','radiatorscript',
    'coolantTankscript','usablescript','partslotscript','partscript','wheelscript','pickupable','electronicsscript',
    'wheelgraphicsscript','dataonmountscript',
    'layerScript','pushablescript','pushgripable','ediblescript','interiorscript',
}


def recover_parameters(env, roots) -> dict:
    source = env
    records = {}
    errors = []
    for root in roots.values():
        for node in source.hierarchy(root,activate_root=True):
            for reader in node['components']:
                if reader.type.name == 'MonoBehaviour':
                    name = source.class_name(reader)
                    if name not in REQUIRED_CLASSES:
                        continue
                    try:
                        fields = source.read(reader)
                    except (EOFError, ValueError) as error:
                        errors.append({'source':source_id(reader),'class':name,'error':str(error)})
                        continue
                    records[f'{key(reader)[0]}:{key(reader)[1]}'] = {'source':source_id(reader),'class':name,'fields':fields}
                elif reader.type.name in {'Rigidbody','WheelCollider','BoxCollider','SphereCollider','CapsuleCollider','MeshCollider','Camera'}:
                    records[f'{key(reader)[0]}:{key(reader)[1]}'] = {'source':source_id(reader),'class':reader.type.name,'fields':source.read(reader)}
    for file,path_id in [('level1',80125),('level1',64896),('level1',64898),('level1',64919)]:
        reader = source.get(file,path_id)
        records[f'{file}:{path_id}'] = {'source':source_id(reader),'class':source.class_name(reader),'fields':source.read(reader)}
    time=source.get('globalgamemanagers',8)
    records['globalgamemanagers:8']={'source':source_id(time),'class':time.type.name,'fields':source.read(time)}
    for path_id,capacity in [(154417,39.),(138629,5.),(144880,5.)]:
        record = records[f'sharedassets1.assets:{path_id}']['fields']
        if record['F']['maxC'] != capacity:
            raise ValueError(f'Tank capacity changed: {path_id}')
        if len(record['F']['fluids']) > 6 or any(fluid['type'] not in range(6) or fluid['amount'] < 0 for fluid in record['F']['fluids']):
            raise ValueError(f'Invalid tank quantities: {path_id}')
    return {'app_id':1017180,'build_id':11140890,'records':records,'byte_checks':source.evidence,'errors':errors,
        'startup_evidence': {
            'static_tanks':'TerrainGenerationSettings.FStart selects StartCar02; no RandomFluid call. Static tanks retain serialized contents.',
            'mass':'massScript.Mass sums active descendant OwnPlusMass. Active tank liters add one kilogram per liter.',
            'wheel_conditions':'partconditionscript.FStart calls StartRandom2. Selected root range is inclusive 1 through 4.',
            'push_force':'enginescript.Start replaces startingPushForce with maxNm * 9.',
        }}
