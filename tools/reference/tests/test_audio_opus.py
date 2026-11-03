import math
import struct
import tempfile
import unittest
import wave
from pathlib import Path

from tools.reference.audio import OPUS_RATE, encode_opus, opus_frames


class AudioOpusTests(unittest.TestCase):
    def test_encode_keeps_length_channels_and_bytes(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            frames = 44100 * 3 + 17
            with wave.open(str(root / 'clip.wav'), 'wb') as clip:
                clip.setnchannels(2); clip.setsampwidth(2); clip.setframerate(44100)
                clip.writeframes(b''.join(struct.pack('<hh', s, -s) for s in
                    (int(9000 * math.sin(i / 20)) for i in range(frames))))
            encode_opus(root / 'clip.wav', root / 'a.ogg', 2, 48000)
            encode_opus(root / 'clip.wav', root / 'b.ogg', 2, 48000)
            self.assertEqual((root / 'a.ogg').read_bytes(), (root / 'b.ogg').read_bytes())
            channels, decoded = opus_frames(root / 'a.ogg')
            self.assertEqual(channels, 2)
            self.assertLessEqual(abs(decoded - frames * OPUS_RATE / 44100), 2)
            self.assertLess((root / 'a.ogg').stat().st_size * 8, (root / 'clip.wav').stat().st_size)
            encode_opus(root / 'clip.wav', root / 'radio.ogg', 2, 32000)
            self.assertLess((root / 'radio.ogg').stat().st_size, (root / 'a.ogg').stat().st_size)


if __name__ == '__main__':
    unittest.main()
