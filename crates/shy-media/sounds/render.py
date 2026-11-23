"""Render shyspace's huddle sounds: soft felt-damped electric piano notes.

Everything is synthesized here from sine waves, with a fixed dither seed, so
the output is the same on every run. Standard library only.

    python3 render.py OUTPUT_DIR
"""

import math
import random
import struct
import sys
import wave
from pathlib import Path

RATE = 48_000

# Loudness targets: the loudest 400 ms window (EBU momentary loudness, LUFS)
# of the mono file. Played on two speakers they measure 3 LU higher.
OWN_LOUDNESS = -27.0
OTHERS_LOUDNESS = -29.5
RING_LOUDNESS = -26.0

ROOT = 220.0
FIFTH = 3 / 2
OCTAVE = 2.0

# Your own arrival spans root to fifth; others' sits a step higher, fifth to
# octave, a little quicker and softer. Arrivals rise, departures mirror them.
# name, notes as multiples of ROOT, seconds between notes, how hard each note
# is played, loudness target, total seconds
CUES = [
    ("joined", [1.0, FIFTH], 0.17, [0.8, 0.95], OWN_LOUDNESS, 1.02),
    ("peer-joined", [FIFTH, OCTAVE], 0.12, [0.7, 0.8], OTHERS_LOUDNESS, 0.84),
    ("peer-left", [OCTAVE, FIFTH], 0.12, [0.7, 0.8], OTHERS_LOUDNESS, 0.84),
    ("left", [FIFTH, 1.0], 0.17, [0.85, 0.9], OWN_LOUDNESS, 1.02),
    ("ring", [1.0, FIFTH, OCTAVE], 0.22, [0.8, 0.85, 0.95], RING_LOUDNESS, 2.5),
]

NOTE_SECONDS = 0.9
NOTE_RING_SECONDS = 0.28
NOTE_ATTACK_SECONDS = 0.012
NOTE_CUTOFF = 2500
DETUNE_CENTS = 1.5

ROOM_LEVEL = 0.16
ROOM_RING_SECONDS = 0.9
ROOM_DELAYS = [1447, 1663, 1879, 2137, 2399, 2671, 2953, 3251]
ROOM_OUTPUT_SIGNS = [1, -1, 1, -1, 1, -1, 1, -1]

TAIL_FADE_SECONDS = 0.35


def seconds_to_samples(seconds):
    return int(round(seconds * RATE))


def smooth_step(position):
    position = min(max(position, 0.0), 1.0)
    return 0.5 - 0.5 * math.cos(math.pi * position)


def one_pole_lowpass(samples, cutoff):
    coefficient = math.exp(-math.tau * cutoff / RATE)
    output = []
    state = 0.0
    for sample in samples:
        state = (1 - coefficient) * sample + coefficient * state
        output.append(state)
    return output


def highpass(samples, cutoff):
    lowpassed = one_pole_lowpass(samples, cutoff)
    return [sample - low for sample, low in zip(samples, lowpassed)]


def biquad(samples, b0, b1, b2, a1, a2):
    output = []
    x1 = x2 = y1 = y2 = 0.0
    for x0 in samples:
        y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        output.append(y0)
        x2, x1 = x1, x0
        y2, y1 = y1, y0
    return output


def felt_keys_note(frequency, velocity):
    """A soft electric piano note: a tone whose brightness fades as it rings,
    with a faint tine at the very start. Two copies a hair apart give a slow
    shimmer."""
    length = seconds_to_samples(NOTE_SECONDS)
    samples = [0.0] * length
    for cents in (-DETUNE_CENTS, DETUNE_CENTS):
        pitch = frequency * 2 ** (cents / 1200)
        for index in range(length):
            time = index / RATE
            angle = math.tau * pitch * time
            brightness = 0.2 + 0.6 * velocity * math.exp(-time / 0.22)
            body = math.exp(-time / NOTE_RING_SECONDS) * math.sin(angle + brightness * math.sin(angle))
            tine_depth = 0.5 * velocity * math.exp(-time / 0.03)
            tine = 0.03 * math.exp(-time / 0.08) * math.sin(angle + tine_depth * math.sin(14 * angle))
            samples[index] += 0.5 * velocity * (body + tine)
    attack = seconds_to_samples(NOTE_ATTACK_SECONDS)
    softened = one_pole_lowpass(samples, NOTE_CUTOFF)
    return [sample * smooth_step(index / attack) for index, sample in enumerate(softened)]


def hadamard(values):
    values = list(values)
    span = 1
    while span < len(values):
        for start in range(0, len(values), span * 2):
            for offset in range(start, start + span):
                first, second = values[offset], values[offset + span]
                values[offset], values[offset + span] = first + second, first - second
        span *= 2
    scale = 1 / math.sqrt(len(values))
    return [value * scale for value in values]


def diffuse(samples, delay, gain):
    line = [0.0] * delay
    output = []
    for index, sample in enumerate(samples):
        delayed = line[index % delay]
        line[index % delay] = sample + gain * delayed
        output.append(delayed - gain * line[index % delay])
    return output


def room(dry):
    """The echo of a small soft room: eight delay lines feeding each other
    evenly, so the tail is smooth rather than metallic."""
    source = [0.0] * seconds_to_samples(0.012) + dry
    source = diffuse(diffuse(source, 241, 0.6), 83, 0.6)
    lines = [[0.0] * delay for delay in ROOM_DELAYS]
    gains = [10 ** (-3 * delay / RATE / ROOM_RING_SECONDS) for delay in ROOM_DELAYS]
    damped = [0.0] * len(ROOM_DELAYS)
    echo = []
    for index, sample in enumerate(source):
        outputs = [line[index % len(line)] for line in lines]
        damped = [0.65 * output + 0.35 * state for output, state in zip(outputs, damped)]
        for line, gain, value in zip(lines, gains, hadamard(damped)):
            line[index % len(line)] = sample + gain * value
        echo.append(sum(sign * output for sign, output in zip(ROOM_OUTPUT_SIGNS, outputs)))
    return highpass(one_pole_lowpass(echo, 4000), 250)


def render_cue(ratios, spacing, velocities, seconds):
    length = seconds_to_samples(seconds)
    dry = [0.0] * length
    for position, (ratio, velocity) in enumerate(zip(ratios, velocities)):
        start = seconds_to_samples(position * spacing)
        for index, sample in enumerate(felt_keys_note(ROOT * ratio, velocity)[: length - start]):
            dry[start + index] += sample
    dry = highpass(highpass(dry, 90), 90)
    return fade_out([sample + ROOM_LEVEL * echo for sample, echo in zip(dry, room(dry))])


def fade_out(samples):
    fade = seconds_to_samples(TAIL_FADE_SECONDS)
    start = len(samples) - fade
    return samples[:start] + [
        sample * smooth_step(1 - (index + 1) / fade) for index, sample in enumerate(samples[start:])
    ]


def k_weighted(samples):
    """ITU-R BS.1770 K-weighting at 48 kHz."""
    shelved = biquad(samples, 1.53512485958697, -2.69169618940638, 1.19839281085285,
                     -1.69065929318241, 0.73248077421585)
    return biquad(shelved, 1.0, -2.0, 1.0, -1.99004745483398, 0.99007225036621)


def max_momentary_loudness(samples):
    window = seconds_to_samples(0.4)
    weighted = k_weighted([0.0] * window + samples + [0.0] * window)
    running = [0.0]
    for value in weighted:
        running.append(running[-1] + value * value)
    step = seconds_to_samples(0.01)
    loudest = max(
        running[start + window] - running[start] for start in range(0, len(weighted) - window, step)
    )
    return -0.691 + 10 * math.log10(loudest / window)


def normalized(samples, target_loudness):
    gain = 10 ** ((target_loudness - max_momentary_loudness(samples)) / 20)
    scaled = [gain * sample for sample in samples]
    peak = max(abs(sample) for sample in scaled)
    if peak > 0.89:
        raise ValueError(f"peak {peak:.2f} would clip at {target_loudness} LUFS")
    return scaled


def write_wav(path, samples, dither_seed):
    dither = random.Random(dither_seed)
    frames = bytearray()
    for sample in samples:
        noise = (dither.random() - dither.random()) / 32768
        frames += struct.pack("<h", max(-32768, min(32767, round((sample + noise) * 32767))))
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(RATE)
        output.writeframes(bytes(frames))


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: render.py OUTPUT_DIR")
    output_dir = Path(sys.argv[1])
    output_dir.mkdir(parents=True, exist_ok=True)
    for dither_seed, (name, ratios, spacing, velocities, loudness, seconds) in enumerate(CUES):
        samples = normalized(render_cue(ratios, spacing, velocities, seconds), loudness)
        write_wav(output_dir / f"{name}.wav", samples, dither_seed)
        print(f"{name}.wav: {len(samples) / RATE:.2f} s")


if __name__ == "__main__":
    main()
