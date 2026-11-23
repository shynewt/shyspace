# Huddle sounds

Soft, felt-damped electric piano notes, picked by the owner. Original work: synthesized from sine waves by `render.py`, with no samples and nothing taken from another product.

- `joined.wav` (you joined, two notes rising A3 → E4) and `left.wav` (you left, E4 → A3): 1.02 s
- `peer-joined.wav` (someone joined, E4 → A4) and `peer-left.wav` (someone left, A4 → E4): 0.84 s, quicker and softer
- `ring.wav`: one 2.5 s cycle of the incoming-huddle ring (A3 → E4 → A4, then ringing out); loop it, it starts and ends at silence

Format: mono, 48 kHz, 16-bit PCM WAV with the plain 44-byte header; 597 KB in total.

Loudness (loudest 400 ms window, measured on the mono file): your own cues −27 LUFS, others' −29.5, ring −26. Play them at unity gain: quiet is intended.

To re-render: `python3 render.py OUTPUT_DIR` (Python standard library only, about 5 s, byte-identical on every run). The notes, timing and loudness targets are constants at the top.
