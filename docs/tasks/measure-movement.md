# Task: measure pilot movement timings from the training video

For a helper agent (e.g. Hermes + Qwen). No Rust needed. The output is one Markdown file.

## Rules (read first)

- Read [CLAUDE.md](../../CLAUDE.md). Two rules matter here. Nothing from `Resources/` is ever committed: no video, no frames, no screenshots. Don't open or run the game's `.exe`/`.dll` files.
- Don't change any code or `.ron` files. Only write `docs/movement/measurements.md`.
- Work on a branch called `measure/movement-timings` and open a pull request. Don't push to `main`.

## Inputs

- `Resources/reference/video/training_full_r1delta_1080p60.mp4`: the whole PC training mission at 60 fps. The wall-run, wall-hang, double jump and mantle lessons are in it.
- `Resources/reference/video/colony_footage.mp4`: Colony gameplay.
- ffmpeg is at `C:\Program Files\Jellyfin\Server\ffmpeg.exe`.
- [docs/movement/values.md](../movement/values.md) lists what each value means.

## Method

At 60 fps one frame is 1/60 s ≈ 0.0167 s. Time things by counting frames, not by eye.

1. Find the moment in the video (mm:ss) for each move below. ffprobe or a scrub through the video works.
2. Dump just that stretch as numbered frames into `Resources/reference/frames/<name>/` (gitignored), for example:
   `ffmpeg -ss 00:04:10 -t 6 -i <video> -vf scale=640:-1 Resources/reference/frames/wallrun1/%04d.png`
3. Find the first and last frame of the move and work out `(last - first) / 60` seconds.
4. Do each measurement **at least 3 times** (different attempts or clips). Report every trial plus the average.

If you can't look at images, write down the video timestamps of each move instead and leave the frame numbers for a person to fill in.

## What to measure

| Name | Start frame | End frame | Notes |
|---|---|---|---|
| `jump_airtime` | feet leave the ground on a standing jump on flat ground | feet land | Then gravity ≈ 8 × 80 / T² (jump height 80 is known from the game files) |
| `double_jump_airtime` | second jump pressed (the view kicks up) | feet land on flat ground | Note the height above the ground when it was pressed, if visible |
| `wallrun_time_limit` | first frame the view tilts onto a long wall | the frame the pilot drops off, on a wall long enough that the run times out | Make sure the run ended by timing out, not at the wall's end |
| `wall_hang_time` | the frame the pilot stops on the wall | slips off (if they hang until it ends) | Skip if the video never hangs until it times out |
| `mantle_time` | hands grab the ledge | standing on top | |
| `ground_accel_time` | first movement from standing still | top walking speed reached (the bob settles) | Rough is fine |

## Output

Write `docs/movement/measurements.md` with:
- one table per measurement: trial number, video file, timestamp, start frame, end frame, seconds;
- the average and the spread (max − min) for each;
- anything odd, e.g. "the training level may use different settings from multiplayer".

Open the PR with a short summary. A person (with Claude) will turn the numbers into `movement.ron` values.
