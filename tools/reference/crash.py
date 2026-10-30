"""Checked crash rule: mainscript crash speeds, fall-off counts and the fall-off power table."""
from .source import key, source_id
from .survival import managed_evidence, finite_record

MAIN=('level1',64919)
# mainscript.defFallOffChance members in order.
CATEGORIES=('none','frontMirror','headlight','sideMirror','napEllenzo','radio','hubcap','wheel','gumi','kmh',
    'coolant','engine','door','bumper','hood','kesztyutarto','seat','foreignfragiletank')
# mainscript.fallOffPowerNeeded by category. A C# static: the scene does not hold it. EVIDENCE proves the values.
FALL_OFF_POWER=(-1.,.2,.2,.2,.2,.2,0.,.6,-1.,.3,.6,.7,.6,.5,.3,.3,.7,.01)
EVIDENCE=[
    'public static float[] fallOffPowerNeeded = new float[18]\n\t{\n'
    '\t\t-1f, 0.2f, 0.2f, 0.2f, 0.2f, 0.2f, 0f, 0.6f, -1f, 0.3f,\n'
    '\t\t0.6f, 0.7f, 0.6f, 0.5f, 0.3f, 0.3f, 0.7f, 0.01f\n\t};',
    'public enum defFallOffChance\n\t{\n'+',\n'.join('\t\t'+c for c in CATEGORIES)+'\n\t}',
    'DamageStuff(Mathf.Abs(speed - lspeed));',
    'v *= crashMultiplier;',
    'HornHit(2f, 5f);',
    'HornHit(0.3f, 1f);',
    'mainscript.M.player.Death();',
    'crashLerp = Mathf.InverseLerp(mainscript.M.crashSpeedMinFallOff, mainscript.M.crashSpeedMaxFallOffMax, v);',
    'item.GetComponent<carscript>().DamageStuff(power * 5f);',
    'lerp > mainscript.fallOffPowerNeeded[(int)fallOffCategory] * 0.8f',
    'slot.UnCraft(_fallOff: true);']
RANGES={'blood':('crashSpeedMinBlood','crashSpeedMaxBlood'),'horn':('crashSpeedMinHorn','crashSpeedMaxHorn'),
    'fall_off':('crashSpeedMinFallOff','crashSpeedMaxFallOff')}
COUNTS=(('minNumFallOff','maxNumFallOff'),('minNumFallOff2','maxNumFallOff2'))


def recover_crash(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    record={'source':source_id(main),**{k:[d[a],d[b]] for k,(a,b) in RANGES.items()},
        'fall_off_max':d['crashSpeedMaxFallOffMax'],'horn_chance':d['crashHornChance'],
        'count':[[d[a],d[b]] for a,b in COUNTS],'fall_off_power':list(FALL_OFF_POWER)}
    print(f"Exported crash rule: fall-off {record['fall_off']}, counts {record['count']}",flush=True)
    return record


def validate_crash(record, manifest) -> None:
    from .acquisition import _shape,_source,_integer,_number,_list
    _shape(record,'source blood horn fall_off fall_off_max horn_chance count fall_off_power')
    finite_record(record)
    _source(record['source'])
    for f in RANGES:
        low,high=_list(record[f])
        _number(low);_number(high)
        if low>high:raise ValueError(f'Crash {f} range is reversed')
    _number(record['fall_off_max']);_number(record['horn_chance'])
    for pair in _list(record['count']):
        low,high=_list(pair)
        _integer(low);_integer(high)
        if low>high:raise ValueError('Crash fall-off count range is reversed')
    if len(record['count'])!=2:raise ValueError('Crash fall-off count ranges differ')
    power=_list(record['fall_off_power'])
    if len(power)!=len(FALL_OFF_POWER):raise ValueError('Crash fall-off power table length differs')
    for p in power:_number(p)
