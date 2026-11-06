"""Synthesize the CC0 weather clips in `added_audio`. Needs numpy and ffmpeg with libopus.

Run from the repository root: python -m tools.reference.synth_weather
The loops are periodic, so the end joins the start with no click.
"""
import subprocess
import tempfile
import wave
from pathlib import Path

import numpy as np

RATE = 48000
OUTPUT = Path(__file__).parent / 'added_audio'


def shaped_noise(random, frames, shape):
    """Periodic noise. `shape(frequency)` gives the amplitude of each FFT bin."""
    spectrum = np.fft.rfft(random.standard_normal(frames))
    spectrum *= shape(np.fft.rfftfreq(frames, 1 / RATE))
    return np.fft.irfft(spectrum, frames)


def band(low, high, slope=2):
    """A soft band pass with `slope` order edges."""
    def shape(f):
        f = np.maximum(f, 1.)
        return 1 / (1 + (low / f) ** slope) / (1 + (f / high) ** slope)
    return shape


def drops(random, frames, count, pitch, decay, wrap):
    """Short decaying sine taps at random times. A periodic loop wraps the tails."""
    out = np.zeros(frames)
    length = int(decay * 6 * RATE)
    t = np.arange(length) / RATE
    for start in random.integers(0, frames, count):
        f = random.uniform(*pitch)
        tap = np.sin(2 * np.pi * f * t) * np.exp(-t / decay) * random.uniform(.2, 1.)
        index = (start + np.arange(length))
        if wrap:
            np.add.at(out, index % frames, tap)
        else:
            keep = index < frames
            out[index[keep]] += tap[keep]
    return out


def normalize(signal, peak):
    return signal * (peak / np.max(np.abs(signal)))


def rain(random, seconds, inside):
    frames = int(seconds * RATE)
    channels = []
    for _ in range(2):
        if inside:
            # Rain on a roof: muffled wash and dull taps.
            wash = shaped_noise(random, frames, band(60, 900))
            taps = drops(random, frames, int(seconds * 90), (180, 700), .012, True)
            signal = wash + .5 * normalize(taps, np.std(wash) * 6)
        else:
            # Rain outside: bright hiss and many small splashes.
            wash = shaped_noise(random, frames, band(400, 9000, 1.5))
            taps = drops(random, frames, int(seconds * 400), (1500, 6000), .003, True)
            signal = wash + .4 * normalize(taps, np.std(wash) * 5)
        # A slow periodic swell keeps the loop from sounding static.
        phase = random.uniform(0, 2 * np.pi)
        swell = 1 + .15 * np.sin(2 * np.pi * np.arange(frames) / frames * 2 + phase)
        channels.append(signal * swell)
    stereo = np.stack(channels, 1)
    return normalize(stereo, .5)


def thunder(random, seconds, crack, delay):
    """A rumble of several decaying low bursts. `crack` adds a sharp close strike."""
    frames = int(seconds * RATE)
    t = np.arange(frames) / RATE
    rumble = shaped_noise(random, frames, band(25, 220, 2))
    envelope = np.zeros(frames)
    for _ in range(6):
        start = delay + random.exponential(.6)
        rise = np.clip((t - start) / .08, 0, 1)
        envelope += rise * np.exp(-np.maximum(t - start, 0) / random.uniform(.6, 1.6)) * random.uniform(.4, 1.)
    signal = rumble * envelope
    if crack:
        burst = shaped_noise(random, frames, band(300, 6000, 1)) * np.exp(-np.maximum(t - delay, 0) / .12) * (t >= delay)
        signal = normalize(signal, 1.) + .6 * normalize(burst, 1.)
    # The tail fades to silence before the end.
    signal *= np.clip((seconds - t) / 1.5, 0, 1)
    return normalize(signal, .8)[:, None]


def write_ogg(name, signal, bit_rate):
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp) / 'clip.wav'
        with wave.open(str(path), 'wb') as clip:
            clip.setnchannels(signal.shape[1]); clip.setsampwidth(2); clip.setframerate(RATE)
            clip.writeframes((np.clip(signal, -1, 1) * 32767).astype('<i2').tobytes())
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', str(path), '-c:a', 'libopus', '-b:a', str(bit_rate),
            '-fflags', '+bitexact', '-map_metadata', '-1', str(OUTPUT / name)], check=True)


def main():
    random = np.random.default_rng(20261106)
    OUTPUT.mkdir(exist_ok=True)
    write_ogg('rain-outside.ogg', rain(random, 8, False), 64000)
    write_ogg('rain-inside.ogg', rain(random, 8, True), 48000)
    write_ogg('thunder-1.ogg', thunder(random, 6, True, .05), 32000)
    write_ogg('thunder-2.ogg', thunder(random, 7, False, .2), 32000)
    write_ogg('thunder-3.ogg', thunder(random, 8, False, .5), 32000)


if __name__ == '__main__':
    main()
