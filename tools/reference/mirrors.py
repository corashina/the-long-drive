"""Source live mirror globals: mainscript ranges, the mirror texture and the Mirror draw distance setting."""
from .source import key, source_id

SETTINGS = ('level0', 16865)
# settingsscript.tabGraphics.sliders[1]: FMirrorDrawDistance.
SLIDER = ('level0', 18331)


def record(source, selected):
    main=source.get('level1',64919);m=source.read(main)
    texture=source.resolve(main,m['defMirrorTexture']);selected.receipt(texture);t=source.read(texture);s=t['m_TextureSettings']
    # mirrorscript.FStart copies defMirrorTexture: one sample, linear, no mips, bilinear, clamp.
    if (t['m_Dimension'],t['m_AntiAliasing'],t['m_SRGB'],t['m_MipMap'],s['m_FilterMode'],s['m_WrapU'],s['m_WrapV'])!=(2,1,False,False,1,1,1):
        raise ValueError(f'{key(texture)} mirror texture differs')
    settings=source.get(*SETTINGS);slider=source.get(*SLIDER);d=source.read(slider)
    selected.receipt(settings);selected.receipt(slider)
    default=source.read(settings)['defS']['FMirrorDrawDistance']
    # The slider rounds to 10^-roundDigit. Its middle shows sliderHalfValue.
    if key(source.resolve(slider,d['settings']))!=key(settings) or d['defValue']!=default or not (d['useHalf'] and d['round'] and d['roundWithIFToo']) or d['minValue']!=d['sliderMinValue']:
        raise ValueError(f'{key(slider)} mirror draw distance slider differs')
    return {'source':selected.receipt(main),'min_fov':m['minFov'],'max_fov':m['maxFov'],'min_distance':m['minDist'],'max_distance':m['maxDist'],
        'texture':[t['m_Width'],t['m_Height']],
        'draw_distance':{'source':source_id(slider),'default':default,'minimum':d['sliderMinValue'],'maximum':d['sliderMaxValue'],
            'half':d['sliderHalfValue'],'limit':d['maxValue'],'step':10**-d['roundDigit']}}
