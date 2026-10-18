//! Auto text: a HUD label that rewrites itself every frame from game
//! state (`SetAutoText( text, HATT_*, value )`): times, team names and
//! scores, the weapon's name and ammo. Ported from client.dll's
//! `HudAutoText_*` classes (factory 0x1804163d0, per-frame pass 0x180414bb0,
//! each class's update beside it; docs/port/hud.md "Auto text").
//!
//! Numbers are printed the way the game does, into fixed buffers that
//! silently cut them (`%d` into 3 bytes keeps 2 digits).

use crate::{ClientVm, Host};

/// One label's auto text.
#[derive(Clone, Debug)]
pub struct AutoText {
    pub kind: Kind,
    /// `EnableAutoText`/`DisableAutoText` (on when set).
    pub enabled: bool,
    /// The script's text (a localization token with %s1.. for the values);
    /// the countdowns' two alternates are always "" from script.
    pub template: String,
    /// The script's number: a start or end time for the clocks.
    pub value: f32,
    /// The last values shown, to skip unchanged frames.
    last: Vec<i64>,
}

/// What a label shows, by `HATT_*` number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 1: time since `value`, "#HudAutoText_Uptime" with minutes, seconds.
    Uptime,
    /// 2: time until `value`, "#HudAutoText_CountdownTime".
    CountdownTime,
    FriendlyTeamName,
    EnemyTeamName,
    /// 5/8 and 6/9: a team's score (round score for 8 and 9).
    FriendlyTeamScore {
        round: bool,
    },
    EnemyTeamScore {
        round: bool,
    },
    FriendlyVsEnemyTeamScore {
        round: bool,
    },
    /// 11/12: the active weapon's name / short name, in the script's text.
    ActiveWeaponName {
        short: bool,
    },
    ActiveWeaponClipAmmo,
    ActiveWeaponMaxAmmo,
    ActiveWeaponMagazineCount,
    OffhandWeaponName(usize),
    OffhandWeaponAmmo(usize),
    /// The localized countdowns: the game's kind number (27–40) and the
    /// clock (game time below 34, real time from 34).
    LocalizedCountdown(i32),
}

impl Kind {
    /// The kind for a `HATT_*` number, with the fixed template some kinds
    /// use instead of the script's text (factory 0x1804163d0). None for an
    /// unsupported number (an error), Some(None) for HATT_NONE.
    #[allow(clippy::type_complexity)]
    fn from_hatt(n: i32) -> Option<Option<(Kind, Option<&'static str>)>> {
        use Kind::*;
        let k = match n {
            0 => return Some(None),
            1 => Uptime,
            2 => CountdownTime,
            3 => FriendlyTeamName,
            4 => EnemyTeamName,
            5 => FriendlyTeamScore { round: false },
            6 => EnemyTeamScore { round: false },
            7 => FriendlyVsEnemyTeamScore { round: false },
            8 => FriendlyTeamScore { round: true },
            9 => EnemyTeamScore { round: true },
            10 => FriendlyVsEnemyTeamScore { round: true },
            11 => ActiveWeaponName { short: false },
            12 => ActiveWeaponName { short: true },
            13 => ActiveWeaponClipAmmo,
            14 => ActiveWeaponMaxAmmo,
            15 => ActiveWeaponMagazineCount,
            16 => OffhandWeaponName(0),
            17 => OffhandWeaponName(1),
            18 => OffhandWeaponAmmo(0),
            19 => OffhandWeaponAmmo(1),
            // The evac clocks are seconds (27) or seconds.ms (29) with a
            // fixed text.
            20 => {
                return Some(Some((
                    LocalizedCountdown(27),
                    Some("#HudAutoText_CheckHudEvacEta"),
                )));
            }
            21 => {
                return Some(Some((
                    LocalizedCountdown(27),
                    Some("#HudAutoText_FriendlyEvacHereLeavingTime"),
                )));
            }
            22 => {
                return Some(Some((
                    LocalizedCountdown(27),
                    Some("#HudAutoText_EnemyEvacHereLeavingTime"),
                )));
            }
            23 => {
                return Some(Some((
                    LocalizedCountdown(29),
                    Some("#HudAutoText_EtaCountdownTime"),
                )));
            }
            24 => {
                return Some(Some((
                    LocalizedCountdown(29),
                    Some("#HudAutoText_LeavingCountdownTime"),
                )));
            }
            // 30 (hours:minutes:seconds) is built as 29 in the game.
            30 => LocalizedCountdown(29),
            27..=40 => LocalizedCountdown(n),
            // 25/26 (matchmaking search) and 41/42 aren't ported yet.
            _ => return None,
        };
        Some(Some((k, None)))
    }
}

/// `snprintf` into a buffer of `size` bytes: at most `size - 1` characters.
fn cut(s: String, size: usize) -> String {
    s.chars().take(size - 1).collect()
}

/// `ceil`, as the game does it for these clocks (truncate, then +1 for a
/// positive fraction), on a 32-bit float.
fn ceil(t: f32) -> i32 {
    let i = t as i32;
    if i != i32::MIN && i as f32 != t && t > 0.0 {
        i + 1
    } else {
        i
    }
}

/// `floor` the same way.
fn floor(t: f32) -> i32 {
    let i = t as i32;
    if i != i32::MIN && i as f32 != t && t < 0.0 {
        i - 1
    } else {
        i
    }
}

/// The text a label should show now: (template, arguments), or a plain
/// text, or None to leave it as it is.
enum Show {
    Localized(String, Vec<String>),
    Plain(String),
}

impl AutoText {
    pub fn new(kind: Kind, template: &str, value: f32) -> AutoText {
        AutoText {
            kind,
            enabled: true,
            template: template.to_string(),
            value,
            last: Vec::new(),
        }
    }

    /// Only show when these values changed (the game's caches start at
    /// INT_MAX, so the first frame always shows).
    fn changed(&mut self, now: Vec<i64>) -> bool {
        if self.last == now {
            return false;
        }
        self.last = now;
        true
    }

    fn update(&mut self, host: &dyn Host, game_time: f32, real_time: f32) -> Option<Show> {
        let player = host.local_player().and_then(|id| host.entity(id));
        let none = i32::MAX as i64;
        let number = |v: i64| cut(format!("{v}"), 10);
        match self.kind {
            // 0x180413310 / 0x180413400: always rewritten.
            Kind::Uptime | Kind::CountdownTime => {
                let t = if self.kind == Kind::Uptime {
                    game_time - self.value
                } else {
                    self.value - game_time
                };
                let s = ceil(t).max(0);
                let token = if self.kind == Kind::Uptime {
                    "#HudAutoText_Uptime"
                } else {
                    "#HudAutoText_CountdownTime"
                };
                Some(Show::Localized(
                    token.into(),
                    vec![
                        cut(format!("{}", s / 60), 3),
                        cut(format!("{:02}", s % 60), 3),
                    ],
                ))
            }
            // 0x1804134f0 / 0x1804135a0: rewritten when the name changes.
            Kind::FriendlyTeamName | Kind::EnemyTeamName => {
                let p = player?;
                let team = if self.kind == Kind::FriendlyTeamName {
                    p.team
                } else {
                    host.other_team(p.team)
                };
                let name = host.team(team).name;
                let key = name.bytes().map(i64::from).collect();
                if !self.changed(key) {
                    return None;
                }
                let token = if self.kind == Kind::FriendlyTeamName {
                    "#HudAutoText_FriendlyTeamName"
                } else {
                    "#HudAutoText_EnemyTeamName"
                };
                Some(Show::Localized(token.into(), vec![name]))
            }
            // 0x180413670 / 0x180413750 / 0x180413850.
            Kind::FriendlyTeamScore { round }
            | Kind::EnemyTeamScore { round }
            | Kind::FriendlyVsEnemyTeamScore { round } => {
                let p = player?;
                let score = |team: i32| {
                    let t = host.team(team);
                    (if round { t.score2 } else { t.score }) as i64
                };
                let (ours, theirs) = (score(p.team), score(host.other_team(p.team)));
                match self.kind {
                    Kind::FriendlyVsEnemyTeamScore { .. } => {
                        if !self.changed(vec![ours, theirs]) {
                            return None;
                        }
                        Some(Show::Localized(
                            "#HudAutoText_FriendlyVsEnemyTeamScore".into(),
                            vec![number(ours), number(theirs)],
                        ))
                    }
                    _ => {
                        let (v, token) = if matches!(self.kind, Kind::FriendlyTeamScore { .. }) {
                            (ours, "#HudAutoText_FriendlyTeamScore")
                        } else {
                            (theirs, "#HudAutoText_EnemyTeamScore")
                        };
                        if !self.changed(vec![v]) {
                            return None;
                        }
                        Some(Show::Localized(token.into(), vec![number(v)]))
                    }
                }
            }
            // 0x180414d30 / 0x180414e50 / 0x180414f70: rewritten when the
            // weapon changes; no weapon shows the Titan or pilot "no weapon"
            // text.
            Kind::ActiveWeaponName { .. } | Kind::OffhandWeaponName(_) => {
                let p = player?;
                let w = match self.kind {
                    Kind::OffhandWeaponName(slot) => host.offhand_weapon(slot),
                    _ => host.active_weapon(),
                };
                let id = w.as_ref().map_or(-1, |w| w.id as i64);
                if !self.changed(vec![id]) {
                    return None;
                }
                Some(match (w, self.kind) {
                    (Some(w), Kind::ActiveWeaponName { short }) => Show::Localized(
                        self.template.clone(),
                        vec![if short {
                            w.short_print_name
                        } else {
                            w.print_name
                        }],
                    ),
                    (Some(w), _) => {
                        let slot = if self.kind == Kind::OffhandWeaponName(0) {
                            0
                        } else {
                            1
                        };
                        Show::Localized(
                            format!("#HudAutoText_OffhandWeaponName{slot}"),
                            vec![w.print_name],
                        )
                    }
                    (None, _) => Show::Plain(if p.is_titan {
                        "#HudAutoText_TitanNoWeaponName".into()
                    } else {
                        "#HudAutoText_PilotNoWeaponName".into()
                    }),
                })
            }
            // 0x1804139b0 / 0x180413a80 / 0x180413b50 / 0x180413c50.
            Kind::ActiveWeaponClipAmmo
            | Kind::ActiveWeaponMaxAmmo
            | Kind::ActiveWeaponMagazineCount
            | Kind::OffhandWeaponAmmo(_) => {
                let p = player?;
                let w = match self.kind {
                    Kind::OffhandWeaponAmmo(slot) => host.offhand_weapon(slot),
                    _ => host.active_weapon(),
                };
                let v = w.map_or(none, |w| match self.kind {
                    Kind::ActiveWeaponMaxAmmo => w.clip_size as i64,
                    Kind::ActiveWeaponMagazineCount => {
                        if w.clip_size == 0 {
                            0
                        } else {
                            (w.reserve_ammo / w.clip_size) as i64
                        }
                    }
                    _ => w.clip_ammo as i64,
                });
                if !self.changed(vec![v]) {
                    return None;
                }
                if v == none {
                    return Some(Show::Plain(String::new()));
                }
                let token = match self.kind {
                    Kind::ActiveWeaponClipAmmo => "#HudAutoText_ActiveWeaponClipAmmo".to_string(),
                    Kind::ActiveWeaponMaxAmmo => "#HudAutoText_ActiveWeaponMaxAmmo".to_string(),
                    Kind::ActiveWeaponMagazineCount => {
                        "#HudAutoText_ActiveWeaponMagazineCount".to_string()
                    }
                    Kind::OffhandWeaponAmmo(slot) => format!(
                        "#HudAutoText_{}OffhandWeaponAmmo{slot}",
                        if p.is_titan { "Titan" } else { "Pilot" }
                    ),
                    _ => unreachable!(),
                };
                Some(Show::Localized(token, vec![number(v)]))
            }
            Kind::LocalizedCountdown(k) => self.countdown(k, game_time, real_time),
        }
    }

    /// 0x180413e10: kinds 27–33 on game time, 34–40 on real time; the same
    /// seven layouts in each half.
    fn countdown(&mut self, kind: i32, game_time: f32, real_time: f32) -> Option<Show> {
        let now = if (34..=40).contains(&kind) {
            real_time
        } else {
            game_time
        };
        let left = self.value - now;
        let t = if left >= 0.0 { left } else { 0.0 };
        let d = |v: i32, size: usize| cut(format!("{v}"), size);
        let d2 = |v: i32| cut(format!("{v:02}"), 3);
        let template = self.template.clone();
        match (kind - 27) % 7 {
            0 => {
                let s = ceil(t).max(0);
                if !self.changed(vec![s as i64]) {
                    return None;
                }
                Some(Show::Localized(template, vec![d(s, 5)]))
            }
            1 => {
                let s = ceil(t).max(0);
                let (m, s) = (s / 60, s % 60);
                if !self.changed(vec![m as i64, s as i64]) {
                    return None;
                }
                Some(Show::Localized(template, vec![d(m, 3), d2(s)]))
            }
            2 => {
                // Seconds, then thousandths printed with %03d into 3 bytes:
                // two digits show. Rewritten every frame.
                let secs = d(floor(t), 5);
                let ms = (floor(t * 1000.0) as f32) % 1000.0;
                let ms = cut(format!("{:03}", ms as i32), 3);
                Some(Show::Localized(template, vec![secs, ms]))
            }
            3 | 5 => {
                let s = ceil(t).max(0);
                let (h, m, s) = (s / 3600, (s % 3600) / 60, (s % 3600) % 60);
                if !self.changed(vec![h as i64, m as i64, s as i64]) {
                    return None;
                }
                let args = if (kind - 27) % 7 == 3 {
                    vec![d(h, 8), d(m, 3), d(s, 3)]
                } else {
                    vec![d(h, 8), d2(m), d2(s)]
                };
                Some(Show::Localized(template, args))
            }
            _ => {
                let s = ceil(t).max(0);
                let days = s / 86400;
                let h = (s % 86400) / 3600;
                let rest = (s % 86400) % 3600;
                let (m, s) = (rest / 60, rest % 60);
                if !self.changed(vec![days as i64, h as i64, m as i64, s as i64]) {
                    return None;
                }
                let args = if (kind - 27) % 7 == 4 {
                    vec![d(days, 8), d(h, 3), d(m, 3), d(s, 3)]
                } else {
                    vec![d(days, 8), d2(h), d2(m), d2(s)]
                };
                // The alternate templates (for under a day / under an
                // hour) are always empty from script, so the main one is
                // used.
                Some(Show::Localized(template, args))
            }
        }
    }
}

/// The weapon fields auto text reads (from the host).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponHud {
    pub id: crate::EntityId,
    /// `printname` / `shortprintname` (localization tokens).
    pub print_name: String,
    pub short_print_name: String,
    pub clip_ammo: i32,
    /// The clip's size.
    pub clip_size: i32,
    /// Ammo outside the clip.
    pub reserve_ammo: i32,
}

/// Make the auto text for `SetAutoText` (None: HATT_NONE). Errors are the
/// game's.
pub fn create(text: &str, hatt: i32, value: f32) -> Result<Option<AutoText>, String> {
    match Kind::from_hatt(hatt) {
        None => Err(format!("SetAutoText: Unsupported HATT type: {hatt}")),
        Some(None) => Ok(None),
        Some(Some((kind, fixed))) => Ok(Some(AutoText::new(kind, fixed.unwrap_or(text), value))),
    }
}

impl ClientVm {
    /// The per-frame auto text pass (0x180414bb0): newest first, enabled
    /// ones only.
    pub fn autotext_frame(&mut self, game_time: f32, real_time: f32) {
        let Some(hud) = self.hud.clone() else { return };
        let order: Vec<usize> = hud.borrow().auto_text_order.clone();
        for i in order {
            let shown = {
                let mut h = hud.borrow_mut();
                let Some(e) = h.elements.get_mut(i).and_then(Option::as_mut) else {
                    continue;
                };
                let Some(at) = e.auto_text.as_mut().filter(|a| a.enabled) else {
                    continue;
                };
                at.update(&*self.host, game_time, real_time)
            };
            let Some(shown) = shown else { continue };
            let mut h = hud.borrow_mut();
            let panel = h.elements[i].as_ref().map(|e| e.panel).unwrap_or(0);
            if let Some(p) = h.ui.panel_mut(panel) {
                match shown {
                    Show::Localized(t, args) => {
                        p.text = t;
                        p.text_args = args;
                    }
                    Show::Plain(t) => {
                        p.text = t;
                        p.text_args.clear();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_cut_like_the_game() {
        assert_eq!(cut(format!("{:03}", 123), 3), "12");
        assert_eq!(cut(format!("{}", 12345), 5), "1234");
        assert_eq!(ceil(1.2), 2);
        assert_eq!(ceil(2.0), 2);
        assert_eq!(floor(1.7), 1);
    }

    #[test]
    fn hatt_30_is_built_as_29() {
        let a = create("x", 30, 0.0).unwrap().unwrap();
        assert_eq!(a.kind, Kind::LocalizedCountdown(29));
        assert!(create("x", 99, 0.0).is_err());
    }
}
