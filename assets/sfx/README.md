# NumFlow UI sound assets

NumFlow uses selected semantic cues from the **UI SFX Glass** pack. The upstream generated audio is dedicated to the public domain under **CC0 1.0**.

Source: https://github.com/romainsimon/uisfx/tree/main/packages/uisfx/sounds/glass

The repository stores PCM WAV conversions (mono, 22.05 kHz, 16-bit) so the Windows native build can play them directly with `PlaySound` without a heavy audio dependency.

Included semantic cues: toggle-on/off, select, open/close, expand/collapse, drag-start, release, delete, and error. Pointer motion and ordinary clicks intentionally remain silent.
