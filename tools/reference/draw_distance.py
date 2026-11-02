"""Source draw-distance settings. The game resolves them at runtime."""
FIELDS = ('drawDistance', 'fogModifier', 'Cam1FarPlane', 'Cam2NearPlane', 'Cam2FarPlane',
          'TerrainGenDist', 'TerrainRemoveDist', 'DistantTerrainGenDist', 'DistantTerrainRemoveDist',
          'DistantTerrain2GenDist', 'DistantTerrain2RemoveDist',
          'DistantTerrainShader1Near', 'DistantTerrainShader1Far', 'DistantTerrainShader1Amount',
          'DistantTerrainShader2Near', 'DistantTerrainShader2Far', 'DistantTerrainShader2Amount',
          'ObjGenDist', 'ObjRemoveDist', 'MountainGenDist', 'MountainRemoveDist',
          'BuildingGenDist', 'BuildingRemoveDist', 'RoadGenDist',
          'ItemRemoveDist', 'ItemSpawnDist', 'ItemFreezeDist', 'ItemUnFreezeDist')


def record(source, settings):
    """Return both source draw-distance sets and the default setting."""
    return {'source': source,
            'minimum': {name: settings['DDSmin'][name] for name in FIELDS},
            'maximum': {name: settings['DDSmax'][name] for name in FIELDS},
            'default': settings['S']['FDrawDistance']}
