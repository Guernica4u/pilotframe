# Movement values

Every number in [`crates/pf-sandbox/assets/movement.ron`](../../crates/pf-sandbox/assets/movement.ron), where it came from, and how sure we are. Units: game units (~1 inch), seconds, degrees.

**Status: all placeholders.** These are round numbers picked to feel roughly right in the greybox sandbox. None are measured yet. Real values come from frame-by-frame analysis of 360 footage ([PLAN §6](../../PLAN.md)). When a value is measured, change it in `movement.ron` and update its row here with the source (clip links, number of trials) and an error bar.

| Value | Current | Source | Confidence | How to measure |
|---|---|---|---|---|
| `capsule_radius` | 16 | placeholder | none | Doorway/gap widths the pilot fits through |
| `capsule_height` | 72 | placeholder | none | Vents/crouch spaces the pilot fits under |
| `eye_height` | 60 | placeholder | none | Camera height against known prop heights |
| `walk_speed` | 180 | placeholder | none | Time to pass known distances at full stick, no sprint |
| `sprint_speed` | 270 | placeholder | none | Same, sprinting |
| `ground_accel` | 1400 | placeholder | none | Frames from standstill to top speed |
| `ground_friction` | 1600 | placeholder | none | Frames from top speed to stop after releasing the stick |
| `air_accel` | 300 | placeholder | none | Mid-air direction change rate (M5) |
| `gravity` | 750 | placeholder | none | Fit a parabola to jump/fall height over time |
| `jump_height` | 48 | placeholder | none | Apex of a standing jump against known geometry |
| `max_slope_deg` | 45 | placeholder | none | Steepest ramp in the maps you can walk up |
| `step_height` | 18 | placeholder | none | Tallest ledge walked up without jumping |

Not movement tuning, but also unmeasured: the simulation tick rate (60 Hz, `TICK_HZ` in `pf-sandbox`) and the camera field of view (60° vertical).
