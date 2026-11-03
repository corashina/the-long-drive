import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

from tools.reference.lighting import export_texture_batches, region_bytes, texture_chunks, validate_texture_batches
from tools.reference.native_bc import mip_sizes


class TextureChunkTests(unittest.TestCase):
    def test_bc1_chunks_keep_source_blocks_on_block_rows(self):
        width, height = 1024, 512
        count = width.bit_length()
        sizes = mip_sizes(width, height, count, 8)
        raw = bytes(i % 251 for i in range(sum(sizes)))
        obj = SimpleNamespace(m_Width=width, m_Height=height, m_MipCount=count, m_TextureFormat=10,
                              get_image_data=lambda: raw)
        reader = SimpleNamespace(assets_file=SimpleNamespace(name='a', source_container_path='a'), path_id=7)
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp)
            chunks = texture_chunks(reader, output, obj, 'bc1')
            payload = b''.join((output / Path(c['data']['path']).name).read_bytes()[16:] for c in chunks)
            self.assertEqual(payload, raw)
            self.assertTrue(all(c['row'] % 4 == 0 for c in chunks))
            self.assertTrue(all(region_bytes('bc1', c['width'], c['rows']) <= 1024 * 1024 for c in chunks))
            texture = {'source': {'file': 'a', 'path_id': 7}, 'format': 'bc1', 'width': width,
                       'height': height, 'mip_count': count, 'chunks': chunks}
            export_texture_batches(texture, output)
            validate_texture_batches(texture, output)
            descriptor = json.loads((output / Path(texture['batches'][0]['descriptor']['file']).name).read_bytes())
            self.assertEqual(descriptor['format'], 'bc1')


if __name__ == '__main__':
    unittest.main()
