"""Settings screen records: settingsscript tab sliders and toggles of build 11140890."""
import re
from .driving import checked_methods
from .source import source_id

# setting fields of the Settings screen, in RefreshSettings order. Driving and graphics fields
# outside this list have their own records (driving.py) or are not ported.
SLIDERS = ('FFPSLimit','FMouseSensXY','FMouseSensX','FMouseSensY','FMouseSensZ','FMouseMove','FMouseRot',
    'FKeyLookSens','FKeySteerSens','FSound','FFieldOfViewFoot','FFieldOfViewCar','FCrosshairScale')
TOGGLES = ('BEnableFpsLimit','BInvertMouseSensX','BInvertMouseSensY','BInvertMouseSensZ','BInvertMouseMove',
    'BInvertMouseRot','BAutoSave','BDisableAutoSaveInCar','BAutoLoad','BWalkHeadBob','BDriveHeadBob','BHelp',
    'BCrosshair','BOldCrosshair','BAlwaysShowHP','BAlwaysShowFood','BAlwaysShowWater','BAlwaysShowPiss','BAlwaysShowShit')
TABS = {'tabGraphics':'graphics','tabControls':'controls','tabMisc':'misc'}


def recover_settings(source):
    """Each field maps to its tab control by RefreshSettings. Values are slider units: S = value * multiplier."""
    bodies = checked_methods(source)[0]
    refresh = bodies['public void RefreshSettings()']
    sliders = {m[0]:(m[1],int(m[2])) for m in re.findall(r'S\.(\w+) = (tab\w+)\.sliders\[(\d+)\]\.GetValue\(\);',refresh)}
    toggles = {m[0]:(m[1],int(m[2])) for m in re.findall(r'S\.(\w+) = (tab\w+)\.toggles\[(\d+)\]\.toggle\.isOn;',refresh)}
    reader = source.get('level0',16865)
    if source.class_name(reader)!='settingsscript':raise ValueError('Settings owner differs')
    data = source.read(reader)
    def control(tab,kind,index,cls):
        control = source.resolve(reader,data[tab][kind][index])
        if control is None or source.class_name(control)!=cls:raise ValueError(f'Unresolved {tab} {kind}: {index}')
        return control
    names = {}
    for name,(tab,index) in toggles.items():names[source_id(control(tab,'toggles',index,'togglesetting'))['path_id']]=name
    record = {'sliders':{},'toggles':{}}
    for name in SLIDERS:
        tab,index = sliders[name]
        v = source.read(control(tab,'sliders',index,'slidersettingscript'))
        if v['useHalf']:raise ValueError(f'{name} half slider is not ported')
        if not v['minValue']<=v['defValue']<=v['maxValue']:raise ValueError(f'{name} default outside range')
        record['sliders'][name] = {'tab':TABS[tab],'default':v['defValue'],'minimum':v['minValue'],'maximum':v['maxValue'],
            'slider':[v['sliderMinValue'],v['sliderMaxValue']],'whole':bool(v['sliderInt']),
            'round':bool(v['round']),'round_typed':bool(v['roundWithIFToo']),'round_digit':v['roundDigit'],
            'multiplier':v['valueMultipler'] or 1.,'menu_only':bool(v['onlyFromMainMenu'])}
    for name in TOGGLES:
        tab,index = toggles[name]
        v = source.read(control(tab,'toggles',index,'togglesetting'))
        if v['resetChildOnEnable']:raise ValueError(f'{name} child reset is not ported')
        children = [names.get(p['m_PathID']) for p in v['childToggles']]
        if any(c not in TOGGLES for c in children):raise ValueError(f'{name} child toggle is not ported')
        record['toggles'][name] = {'tab':TABS[tab],'default':bool(v['def']),'children':children}
    return record
