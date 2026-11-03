"""Checked authored player poses and IK Start chains. No solver runs here."""
from .source import key, source_id, vector
from .gltf import converted_transform, convert_rotation
from .library.animations import _rotation
from .survival import managed_evidence

ANCHOR_FIELDS = dict(CharacterRoot=29438,CharacterStandPos=36982,CharacterDuckPos=26032,
    defLHPos=34234,defRHPos=30124,defLHPosStand=32632,defRHPosStand=28934,
    defLHPosDuck=25084,defRHPosDuck=28428,defLLPos=36417,defRLPos=36921,
    defdefLLPos=37888,defdefRLPos=25884,RH=23118,TpickedUpParent=39812,
    TpickedUpParent2=23837,TpickedUpPos=39814,usingHand=32168,pointAt=35402,
    CamParent=22249,THeadDir=37709,ghostHead=28987,TStandHeadPos=39823,
    TDuckHeadPos=39849,TProneHeadPos=23339,THeadBob=39808,THeadLean=39829,Th=39811)
TARGET_FIELDS = dict(HHTarget=25362,LHTarget=32234,RHTarget=32762,LLTarget=36286,RLTarget=26378,Pelvis=29799)
WALK_FIELDS = ('LeftLegWalkWayPoints','RightLegWalkWayPoints')
SIT_FIELDS = ('LHSitPose','RHSitPose','LLSitPose','RLSitPose')
NULL_FIELDS = ('nearestHand','nearestLeg','TpickedUp','VCurrentLHT','VCurrentRHT','VCurrentLLT','VCurrentRLT')
IK_IDS = (69323,79008,76627,70472,76733)
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
    return result


