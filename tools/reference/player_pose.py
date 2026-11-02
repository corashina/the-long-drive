"""Checked authored player poses and IK Start chains. No solver runs here."""
import math
from .source import key, source_id, vector
from .gltf import converted_transform, convert_rotation
from .library.animations import _rotation
from .survival import finite_record, managed_evidence

ANCHOR_FIELDS = dict(CharacterRoot=29438,CharacterStandPos=36982,CharacterDuckPos=26032,
    defLHPos=34234,defRHPos=30124,defLHPosStand=32632,defRHPosStand=28934,
    defLHPosDuck=25084,defRHPosDuck=28428,defLLPos=36417,defRLPos=36921,
    defdefLLPos=37888,defdefRLPos=25884,RH=23118,TpickedUpParent=39812,
    TpickedUpParent2=23837,TpickedUpPos=39814,usingHand=32168,pointAt=35402,
    CamParent=22249,THeadDir=37709,ghostHead=28987,TStandHeadPos=39823,
    TDuckHeadPos=39849,TProneHeadPos=23339,THeadBob=39808,THeadLean=39829,Th=39811)
TARGET_FIELDS = dict(HHTarget=25362,LHTarget=32234,RHTarget=32762,LLTarget=36286,RLTarget=26378,Pelvis=29799)
WALK_FIELDS = ('LeftLegWalkWayPoints','RightLegWalkWayPoints')
WALK_IDS = ([22742,32651,34295,22427,37543,24385],[28142,26891,36507,36405,33777,27003])
SIT_FIELDS = ('LHSitPose','RHSitPose','LLSitPose','RLSitPose')
SIT_IDS = ([34069]*3,[26796]*3,[24726,25212,25409],[28329,30177,36962])
NULL_FIELDS = ('nearestHand','nearestLeg','TpickedUp','VCurrentLHT','VCurrentRHT','VCurrentLLT','VCurrentRLT')
IK_IDS = (69323,79008,76627,70472,76733)
IK_BONES = ([26925,26072,30531],[38757,28290,34137],[36277,36234,27719,37236,29226],[32657,37442,37971],[34390,29634,31204])
IK_LINKS = ((29799,36286,23301),(29799,26378,38357),(29799,25362,37866),(38049,32234,28800),(25710,32762,38405))
SCALAR_FIELDS = ('walkLerp','walkRotLerp','walkSpeedModifier','FHeadLerp','FduckLerp','onehandLimit','pickedUpLerp')
ANIM_SCALARS = ('headBackM','headBackLerp','headForwardLerp','maxHeadBobX','maxHeadBobY','maxHeadBobZ','sendInterval')


def resolved_transform(source, reader):
    if reader is None:raise ValueError('Missing required player source link')
    if reader.type.name == 'GameObject':return source.transform_for(reader)
    if reader.type.name not in ('Transform','RectTransform'):
        return source.transform_for(source.resolve(reader,source.read(reader)['m_GameObject']))
    return reader


def recover_player_pose(source, manifest):
    nodes={key(source.get(n['source']['file'],n['source']['path_id'])):n for n in manifest['nodes']}
    def node(reader):
        reader=resolved_transform(source,reader)
        if key(reader) not in nodes:raise ValueError('Player pose link outside retained hierarchy')
        return nodes[key(reader)]['id']
    player=source.get('level1',80125);d=source.read(player)
    anim=source.resolve(player,d['anim']);ad=source.read(anim)
    def links(owner,values):return [node(source.resolve(owner,p)) for p in values]
    anchors={}
    for field in ANCHOR_FIELDS:
        reader=source.resolve(player,d[field]);t=source.read(reader)
        anchors[field]={'source':source_id(reader),'node':node(reader),'transform':converted_transform(t)}
    chains=[]
    for pid in IK_IDS:
        reader=source.get('level1',pid);v=source.read(reader)
        end=source.resolve(reader,v['endBone']);bones=[end]
        if not isinstance(v['chainLength'],int) or not 1<=v['chainLength']<=8:raise ValueError('Player IK chain length')
        for _ in range(v['chainLength']):
            parent=source.resolve(bones[0],source.read(bones[0])['m_Father'])
            if parent is None:raise ValueError('Missing authored IK parent')
            bones.insert(0,parent)
        chains.append({'source':source_id(reader),'owner':node(reader),'target':node(source.resolve(reader,v['target'])),
            'pole':node(source.resolve(reader,v['pole'])) if v['pole']['m_PathID'] else None,
            'end_bone':node(end),'bones':[node(b) for b in bones],'chain_length':v['chainLength'],
            'use_pole':bool(v['usePole']),'force_end':bool(v['forceEnd']),'stretch':bool(v['stretch']),
            'offsets':list(convert_rotation(_rotation(vector(v['offsets']))))})
    follow=source.get('level1',74950);fd=source.read(follow)
    if len(fd['T'])!=len(fd['Target']):raise ValueError('Unequal player follow arrays')
    result={'follow':{'source':source_id(follow),'owner':node(follow),'enabled':bool(fd['m_Enabled']),'inverse':bool(fd['inv']),
        'nodes':links(follow,fd['T']),'targets':links(follow,fd['Target'])},'player_source':source_id(player),'animation_source':source_id(anim),'root':node(player),
        'anchors':anchors,'targets':{f:node(source.resolve(anim,ad[f])) for f in TARGET_FIELDS},
        'walk_paths':[links(player,d[f]) for f in WALK_FIELDS],
        'sit_poses':[links(player,d[f]) for f in SIT_FIELDS],
        'wearable_anchors':links(player,d['TWearablePos']),
        'character_meshes':links(player,d['CharacterMesh']),
        'runtime_targets':{f:node(source.resolve(player,d[f])) if d[f]['m_PathID'] else None for f in NULL_FIELDS},
        'walk_offsets':[d['LeftLegWalkOffset'],d['RightLegWalkOffset']],
        'method_inputs':dict(VLerpSpeedHand=20.,VLerpSpeedLeg=20.,VMoveLockTime=.5,handSnapDelay=.25),'scalars':{f:d[f] for f in SCALAR_FIELDS},'animation_scalars':{f:ad[f] for f in ANIM_SCALARS},
        'right_hand_rotation':list(convert_rotation(_rotation(vector(d['RHRotSnapHand'])))),
        'use_override_sit_pose':bool(d['useOverrideSitPose']),'override_sit_pose_index':d['overrideSitposeIndex'],
        'ik_chains':chains,'evidence':managed_evidence(source,['bones[num] = bones[num + 1].parent;',
            'bones[j].parent = transform;', 'private float VLerpSpeedHand = 20f;', 'private float VLerpSpeedLeg = 20f;', 'private float VMoveLockTime = 0.5f;', 'private float handSnapDelay = 0.25f;', 'MoveLegsWalk();', 'public void MoveNearestHand(',
            'public void MoveNearestLeg(', 'T[j].position = Target[j].position;', 'T[j].rotation = Target[j].rotation;'])}
    validate_player_pose(result,manifest)
    return result


def validate_player_pose(p,manifest):
    from .clothing import descendant
    finite_record(p)
    nodes={n['id']:n for n in manifest['nodes']}
    def sid(i):return {'file':'level1','path_id':i}
    def match(n,i):return nodes.get(n,{}).get('source')==sid(i) and descendant(nodes,n,p['root'])
    if p['player_source']!=sid(80125) or p['animation_source']!=sid(68762) or not match(p['root'],39843):raise ValueError('Player pose identity')
    asset=next(a for a in manifest['assets'] if a['id']==nodes[p['root']]['asset'])
    if sum(a['usage']=='player_prototype' for a in manifest['assets'])!=1 or asset['usage']!='player_prototype' or len([n for n in nodes.values() if n['asset']==asset['id']])!=384:raise ValueError('Complete inactive Player hierarchy')
    if set(p['anchors'])!=set(ANCHOR_FIELDS) or set(p['targets'])!=set(TARGET_FIELDS):raise ValueError('Complete player pose fields')
    for f,i in ANCHOR_FIELDS.items():
        a=p['anchors'][f]
        if a['source']!=sid(i) or not match(a['node'],i) or a['transform']!=nodes[a['node']]['transform']:raise ValueError('Player anchor pose')
    if any(not match(p['targets'][f],i) for f,i in TARGET_FIELDS.items()):raise ValueError('Player target links')
    for field,expected in [('walk_paths',WALK_IDS),('sit_poses',SIT_IDS)]:
        if len(p[field])!=len(expected):raise ValueError('Player pose arrays')
        for actual,ids in zip(p[field],expected,strict=True):
            if len(actual)!=len(ids) or any(not match(n,i) for n,i in zip(actual,ids,strict=True)):raise ValueError('Player pose array links')
    if len(p['wearable_anchors'])!=5 or any(not match(n,i) for n,i in zip(p['wearable_anchors'],(26477,28766,35477,36615,39162),strict=True)):raise ValueError('Player wearable anchors')
    if p['runtime_targets']!={f:None for f in NULL_FIELDS}:raise ValueError('Authored null player targets')
    if p['method_inputs']!={'VLerpSpeedHand':20.,'VLerpSpeedLeg':20.,'VMoveLockTime':.5,'handSnapDelay':.25}:raise ValueError('Player method inputs')
    if p['walk_offsets']!=[.5,0.] or p['scalars']!={'walkLerp':5.,'walkRotLerp':1.,'walkSpeedModifier':0.10000000149011612,'FHeadLerp':15.,'FduckLerp':5.,'onehandLimit':10.,'pickedUpLerp':6.}:raise ValueError('Player pose inputs')
    head={f:v for f,v in p['animation_scalars'].items() if f!='sendInterval'}
    if head!={'headBackM':0.10000000149011612,'headBackLerp':10.,'headForwardLerp':4.,'maxHeadBobX':0.20000000298023224,'maxHeadBobY':0.10000000149011612,'maxHeadBobZ':0.30000001192092896}:raise ValueError('Player head inputs')
    # `playeranimationscript.sendInterval`: the multiplayer body send period.
    send=p['animation_scalars'].get('sendInterval')
    if not isinstance(send,float) or not math.isfinite(send) or send<=0:raise ValueError('Player send interval')
    if p['use_override_sit_pose'] or p['override_sit_pose_index']!=0 or len(p['character_meshes'])!=12 or any(not match(n,i) for n,i in zip(p['character_meshes'],(32922,38614,23205,27836,22144,30679,23000,31080,39486,32302,30051,29368),strict=True)):raise ValueError('Player view or sit inputs')
    if any(abs(a-b)>1e-6 for a,b in zip(p['right_hand_rotation'],convert_rotation(_rotation((0,0,-90))),strict=True)):raise ValueError('Exact player hand rotation')
    if len(p['right_hand_rotation'])!=4 or abs(sum(v*v for v in p['right_hand_rotation'])-1)>.002:raise ValueError('Player hand rotation')
    if len(p['ik_chains'])!=5:raise ValueError('Complete player IK chains')
    for index,(c,bones,links) in enumerate(zip(p['ik_chains'],IK_BONES,IK_LINKS,strict=True)):
        if c['source']!=sid(IK_IDS[index]) or c['chain_length']!=len(bones)-1 or len(c['bones'])!=len(bones):raise ValueError('Player IK chain identity')
        if any(not match(c[f],i) for f,i in zip(('owner','target','pole'),links,strict=True)) or not match(c['end_bone'],bones[-1]) or any(not match(n,i) for n,i in zip(c['bones'],bones,strict=True)):raise ValueError('Player IK links')
        if not c['use_pole'] or c['force_end']!=(index==2) or c['stretch']!=(index>=2):raise ValueError('Player IK flags')
        for child,parent in zip(c['bones'],[c['owner'],*c['bones'][:-1]],strict=True):
            if nodes[child]['parent']!=parent:raise ValueError('Authored player IK hierarchy')
        for n in c['bones'][1:]:
            if math.sqrt(sum(v*v for v in nodes[n]['transform']['translation']))<=1e-8:raise ValueError('Zero-length player IK segment')
        expected_rotation=convert_rotation(_rotation((0,-90,0) if index<2 else (0,90,20) if index==2 else (0,0,0)))
        if len(c['offsets'])!=4 or any(abs(a-b)>1e-6 for a,b in zip(c['offsets'],expected_rotation,strict=True)):raise ValueError('Exact player IK offsets')
        if len(c['offsets'])!=4 or abs(sum(v*v for v in c['offsets'])-1)>.002:raise ValueError('Player IK rotation')

    follow=p['follow']
    if follow['source']!=sid(74950) or not match(follow['owner'],31145) or not follow['enabled'] or follow['inverse'] or len(follow['nodes'])!=66 or len(follow['targets'])!=66:raise ValueError('Player follow identity')
    for field,expected in [('nodes',FOLLOW_NODES),('targets',FOLLOW_TARGETS)]:
        if any(not match(n,i) for n,i in zip(follow[field],expected,strict=True)):raise ValueError('Player follow links')

FOLLOW_NODES = [25455, 30383, 37943, 37865, 28293, 39700, 32025, 36794, 39866, 24604, 32753, 27265, 24960, 22453, 31901, 32533, 33876, 27041, 24970, 38256, 24859, 35588, 38783, 27677, 31650, 25383, 38246, 24558, 35551, 24975, 35109, 30279, 38321, 37965, 34393, 22219, 38189, 22159, 33593, 37927, 31634, 31273, 23075, 22202, 38190, 37896, 36151, 28886, 22054, 27730, 25376, 26108, 36646, 36480, 33594, 27782, 28693, 25452, 37612, 38100, 36137, 39393, 27722, 36856, 37937, 32014]
FOLLOW_TARGETS = [26068, 29799, 36277, 36234, 27719, 38049, 32657, 37442, 38141, 26362, 32161, 28124, 25617, 24366, 33505, 31941, 35609, 28814, 25713, 40017, 26804, 33934, 39475, 28176, 33387, 25893, 40029, 26243, 33814, 25710, 34390, 29634, 40075, 36258, 35131, 24021, 39954, 23985, 35295, 36207, 33366, 29479, 23595, 23917, 39951, 36203, 37854, 27055, 23832, 28283, 26010, 25415, 37352, 37236, 35291, 28365, 26925, 26072, 35908, 39681, 37746, 38757, 28290, 36314, 36283, 32635]
