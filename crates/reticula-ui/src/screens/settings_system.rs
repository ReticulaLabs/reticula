//! Settings sub-menu: System — the device clock.
//!
//! Shows the current UTC date and time and lets it be corrected by hand.
//!
//! This matters more than a normal clock screen. Reticulum stamps every
//! announce with the local time, and peers compare that against the last one
//! they saw for the same destination: an announce that looks *older* is
//! discarded as stale. So a device that boots without a clock does not merely
//! display the wrong time, it goes invisible to the network, and stays that way
//! until it recovers one. Being able to correct it by hand is the difference
//! between a node that has to be carried outside and one that doesn't.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use reticula_hal::KeyCode;

use crate::command::Command;
use crate::context::ViewContext;
use crate::screens::ListState;
use crate::theme::Theme;
use crate::timefmt::{self, is_implausible};
use crate::widgets::{self, px};

/// The editable parts of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
}

impl Field {
    /// Every field, in the order they are entered.
    const ALL: [Field; 6] = [
        Field::Year,
        Field::Month,
        Field::Day,
        Field::Hour,
        Field::Minute,
        Field::Second,
    ];

    fn label(self) -> &'static str {
        match self {
            Field::Year => "Year",
            Field::Month => "Month",
            Field::Day => "Day",
            Field::Hour => "Hour",
            Field::Minute => "Min",
            Field::Second => "Sec",
        }
    }

    /// How many digits the field accepts, so the UI can pre-pad.
    fn width(self) -> usize {
        match self {
            Field::Year => 4,
            Field::Month | Field::Day | Field::Hour | Field::Minute | Field::Second => 2,
        }
    }
}

pub struct SettingsSystemScreen {
    pub state: ListState,
    /// The timestamp being edited, as typed so far.
    draft: [String; 6],
    editing: Option<Field>,
    /// Whether the next keystroke should clear the field first. Set when an
    /// edit opens, because the field starts out pre-filled from the clock.
    replacing: bool,
    /// Whether the draft has been seeded from the live clock.
    seeded: bool,
}

impl Default for SettingsSystemScreen {
    fn default() -> Self {
        Self {
            state: ListState::default(),
            draft: Default::default(),
            editing: None,
            replacing: false,
            seeded: false,
        }
    }
}

impl SettingsSystemScreen {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        if let Some(field) = self.editing {
            let index = Field::ALL.iter().position(|f| *f == field).unwrap_or(0);
            return match key {
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    // The field arrives pre-filled from the clock, so it is
                    // already at full width: appending would make the field
                    // untypable. Clear it on the first keystroke instead, so
                    // the value is replaced rather than edited in place.
                    if self.replacing {
                        self.draft[index].clear();
                        self.replacing = false;
                    }
                    if self.draft[index].len() < field.width() {
                        self.draft[index].push(c);
                    }
                    Command::None
                }
                KeyCode::Char('-') if field == Field::Year && self.draft[index].is_empty() => {
                    self.draft[index].push('-');
                    self.replacing = false;
                    Command::None
                }
                KeyCode::Backspace => {
                    self.replacing = false;
                    self.draft[index].pop();
                    Command::None
                }
                KeyCode::Enter => {
                    self.editing = None;
                    Command::None
                }
                KeyCode::Esc => {
                    // Abandon the edit and re-seed from the live clock, so a
                    // half-typed value never leaks into the apply step.
                    self.editing = None;
                    self.seeded = false;
                    Command::None
                }
                _ => Command::None,
            };
        }

        match key {
            KeyCode::Up => {
                self.state.move_up();
                Command::None
            }
            KeyCode::Down => {
                self.state.move_down(Field::ALL.len() + 1);
                Command::None
            }
            KeyCode::Enter => {
                if let Some(field) = Field::ALL.get(self.state.selected) {
                    self.editing = Some(*field);
                    // The field still holds the seeded value; the first
                    // keystroke should replace it wholesale.
                    self.replacing = true;
                    return Command::None;
                }
                self.apply()
            }
            KeyCode::Esc => Command::Back,
            _ => Command::None,
        }
    }

    /// Turn the draft into a timestamp, if it is a real one.
    fn draft_unix(&self) -> Option<u64> {
        let n = |i: usize| self.draft[i].parse::<i64>().ok();
        timefmt::to_unix(
            n(0)?,
            n(1)? as u32,
            n(2)? as u32,
            n(3)? as u32,
            n(4)? as u32,
            n(5)? as u32,
        )
    }

    /// Emit a [`Command::SetTime`] for the drafted timestamp.
    ///
    /// An unparseable draft does nothing: silently setting a wrong clock is
    /// worse than ignoring the keypress, because a wrong-but-plausible clock
    /// gets this node's announces *accepted* and then judged stale once real
    /// time moves past them.
    fn apply(&mut self) -> Command {
        // `validate` first, so the reason is available to render and a bad
        // draft can never reach the clock even if `draft_unix` were to accept
        // it.
        if validate(&self.draft).is_err() {
            return Command::None;
        }
        match self.draft_unix() {
            Some(unix) => Command::SetTime(unix),
            None => Command::None,
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let size = target.bounding_box().size;
        let width = size.width as i32;
        let height = size.height as i32;

        // Re-seed from the live clock on first render and whenever the user
        // escapes an edit, so the draft tracks the clock while nothing is being
        // typed.
        if !self.seeded {
            self.seeded = true;
            self.seed(ctx);
        }

        widgets::draw_header(target, width, "System", "", &ctx.network, theme).ok();

        let mut y = theme.header_h;

        // Current time, and a loud flag when it is not believable.
        match ctx.system_time {
            Some(now) if is_implausible(now) => {
                let line = timefmt::format_datetime(now);
                widgets::draw_text(target, Point::new(0, y), &line, theme.danger, theme).ok();
                y += theme.line_h;
                widgets::draw_text(
                    target,
                    Point::new(0, y),
                    "UNSET — peers reject our announces",
                    theme.danger,
                    theme,
                )
                .ok();
            }
            Some(now) => {
                let line = format!(
                    "{} UTC  {}",
                    timefmt::format_datetime(now),
                    timefmt::format_weekday(now)
                );
                widgets::draw_text(target, Point::new(0, y), &line, theme.text, theme).ok();
            }
            None => {
                widgets::draw_text(
                    target,
                    Point::new(0, y),
                    "clock unavailable",
                    theme.danger,
                    theme,
                )
                .ok();
            }
        }
        y += theme.line_h + 4;

        // Editable date/time fields.
        for (i, field) in Field::ALL.iter().enumerate() {
            let value = widgets::truncate(&self.draft[i], field.width());
            let label = format!("{}: {}", field.label(), value);
            let at = Point::new(0, y);
            let is_editing = self.editing == Some(*field);
            if self.state.selected == i || is_editing {
                widgets::draw_highlight(
                    target,
                    at,
                    &label,
                    width,
                    theme.selection,
                    theme.selection_text,
                    theme,
                )
                .ok();
            } else {
                widgets::draw_text(target, at, &label, theme.text, theme).ok();
            }
            y += theme.line_h;
        }

        // Last row: apply.
        let at = Point::new(0, y);
        if self.state.selected == Field::ALL.len() {
            widgets::draw_highlight(
                target,
                at,
                "Apply time",
                width,
                theme.selection,
                theme.selection_text,
                theme,
            )
            .ok();
        } else {
            widgets::draw_text(target, at, "Apply time", theme.text, theme).ok();
        }
        y += theme.line_h;

        // An invalid draft is worth saying out loud, since Enter does nothing.
        if self.editing.is_none() && self.state.selected == Field::ALL.len() {
            if let Err(reason) = validate(&self.draft) {
                widgets::draw_text(target, Point::new(0, y), reason, theme.danger, theme).ok();
            }
        }

        if !ctx.notice.is_empty() {
            y += theme.line_h;
            widgets::draw_text(target, Point::new(0, y), ctx.notice, theme.ok, theme).ok();
        }

        let footer = Rectangle::new(
            Point::new(0, height - theme.line_h),
            px(width, theme.line_h),
        );
        let short = widgets::truncate(ctx.identity_hex, 12);
        widgets::draw_bar(
            target,
            footer,
            "ALT+Backspace back",
            &short,
            theme.surface,
            theme.text_dim,
            theme,
        )
        .ok();
    }

    /// Populate the draft from the live clock.
    ///
    /// With no clock at all, start from the plausibility floor rather than
    /// 1970, so the first manual entry is a near-sane value the user only has
    /// to correct by a few years.
    fn seed(&mut self, ctx: &ViewContext) {
        let dt = ctx
            .system_time
            .filter(|t| !is_implausible(*t))
            .map(timefmt::from_unix)
            .unwrap_or(timefmt::from_unix(timefmt::PLAUSIBLE_UNIX_TIME));

        self.draft = [
            dt.year.to_string(),
            pad2(dt.month),
            pad2(dt.day),
            pad2(dt.hour),
            pad2(dt.minute),
            pad2(dt.second),
        ];
    }
}

fn pad2(v: u32) -> String {
    format!("{v:02}")
}

/// Explain why a draft cannot be applied, if it cannot.
fn validate(draft: &[String; 6]) -> Result<(), &'static str> {
    let parse = |i: usize| draft[i].parse::<i64>();
    for (i, field) in Field::ALL.iter().enumerate() {
        if draft[i].is_empty() {
            return Err("all fields are required");
        }
        if parse(i).is_err() {
            return Err(match field {
                Field::Year => "year must be a number",
                _ => "value must be a number",
            });
        }
    }

    let year = parse(0).unwrap_or(0);
    let month = parse(1).unwrap_or(0) as u32;
    let day = parse(2).unwrap_or(0) as u32;
    if !(1..=12).contains(&month) {
        return Err("month must be 01-12");
    }
    if day < 1 || day > timefmt::days_in_month(year, month) {
        return Err("day is out of range for that month");
    }
    if parse(3).unwrap_or(-1) > 23 {
        return Err("hour must be 00-23");
    }
    if parse(4).unwrap_or(-1) > 59 {
        return Err("minute must be 00-59");
    }
    if parse(5).unwrap_or(-1) > 59 {
        return Err("second must be 00-59");
    }
    if timefmt::to_unix(year, month, day, 0, 0, 0)
        .map(|t| is_implausible(t))
        .unwrap_or(true)
    {
        return Err("that date is before 2020 — peers will reject it");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> SettingsSystemScreen {
        SettingsSystemScreen::new()
    }

    fn empty_ctx<'a>() -> ViewContext<'a> {
        ViewContext::default()
    }

    fn type_into(s: &mut SettingsSystemScreen, keys: &[KeyCode]) {
        for key in keys {
            s.handle_key(*key);
        }
    }

    fn enter_field(s: &mut SettingsSystemScreen, row: usize) {
        s.state.selected = row;
        s.handle_key(KeyCode::Enter);
        assert!(s.editing.is_some(), "row {row} should enter edit mode");
    }

    /// Replace a field's contents by typing, the way a user would.
    ///
    /// Goes through the same path as a real edit, so the replace-on-first-key
    /// behaviour is exercised rather than bypassed.
    fn retype(s: &mut SettingsSystemScreen, row: usize, digits: &str) {
        enter_field(s, row);
        type_into(s, &chars(digits));
        s.handle_key(KeyCode::Enter);
        assert!(
            s.editing.is_none(),
            "typing should commit and close the edit"
        );
    }

    /// Leave whatever field is being edited, so a test can start the next one.
    fn leave_field(s: &mut SettingsSystemScreen) {
        s.handle_key(KeyCode::Enter);
        assert!(
            s.editing.is_none(),
            "Enter should commit and close the edit"
        );
    }

    fn chars(text: &str) -> Vec<KeyCode> {
        text.chars().map(KeyCode::Char).collect()
    }

    #[test]
    fn entering_a_field_opens_the_keyboard() {
        let mut s = screen();
        s.seed(&empty_ctx());
        // Selection starts on the first field.
        assert_eq!(s.state.selected, 0);
        enter_field(&mut s, 0);
        type_into(&mut s, &chars("20"));
        assert_eq!(s.draft[0], "20");

        // The Apply row must not open an editor.
        leave_field(&mut s);
        s.state.selected = Field::ALL.len();
        assert_eq!(s.handle_key(KeyCode::Enter), Command::None);
        assert!(s.editing.is_none(), "the Apply row must not edit");
    }

    /// A field arrives pre-filled from the clock, so it starts at full width.
    /// The first keystroke must replace the value, not be swallowed by the
    /// width cap — otherwise the field cannot be corrected at all.
    #[test]
    fn the_first_keystroke_replaces_the_seeded_value() {
        let mut s = screen();
        let ctx = ViewContext {
            system_time: Some(1_791_924_423),
            ..empty_ctx()
        };
        s.seed(&ctx);
        assert_eq!(
            s.draft[0], "2026",
            "year should start seeded from the clock"
        );

        enter_field(&mut s, 0);
        type_into(&mut s, &chars("2027"));
        assert_eq!(
            s.draft[0], "2027",
            "typing must replace the pre-filled year, not be dropped"
        );
    }

    /// Only the first keystroke replaces; the rest append as normal.
    #[test]
    fn later_keystrokes_append_to_the_replacement() {
        let mut s = screen();
        s.seed(&empty_ctx());
        // Seeding from the plausibility floor gives 2020-09-13, so month is 09.
        assert_eq!(s.draft[1], "09");

        enter_field(&mut s, 1);
        type_into(&mut s, &chars("0"));
        assert_eq!(s.draft[1], "0", "first key replaces");
        type_into(&mut s, &chars("7"));
        assert_eq!(s.draft[1], "07", "second key appends");
    }

    /// Backspacing a pre-filled field edits it in place, without the blanket
    /// clear — otherwise one Backspace would wipe the whole value.
    #[test]
    fn backspace_edits_the_seeded_value_in_place() {
        let mut s = screen();
        s.seed(&empty_ctx());
        assert_eq!(s.draft[0], "2020");

        enter_field(&mut s, 0);
        s.handle_key(KeyCode::Backspace);
        assert_eq!(s.draft[0], "202", "Backspace removes one digit");
    }

    #[test]
    fn digit_input_is_capped_at_the_field_width() {
        let mut s = screen();
        s.seed(&empty_ctx());
        retype(&mut s, 0, "123456");
        assert_eq!(s.draft[0], "1234", "year caps at 4 digits");
        retype(&mut s, 1, "123456");
        assert_eq!(s.draft[1], "12", "month caps at 2 digits");
    }

    #[test]
    fn backspace_and_escape_edit_the_draft() {
        let mut s = screen();
        s.seed(&empty_ctx());
        enter_field(&mut s, 0);
        // Backspace first, so the pre-filled value is not replaced wholesale.
        s.handle_key(KeyCode::Backspace);
        type_into(&mut s, &chars("2"));
        assert_eq!(s.draft[0], "2022");

        s.handle_key(KeyCode::Esc);
        assert!(s.editing.is_none(), "Esc leaves edit mode");
    }

    #[test]
    fn escape_discards_the_half_typed_draft() {
        let mut s = screen();
        let ctx = ViewContext {
            system_time: Some(1_791_924_423),
            ..empty_ctx()
        };
        s.seeded = false;
        s.seed(&ctx);
        assert_eq!(s.draft[0], "2026");

        retype(&mut s, 0, "1999");
        assert_eq!(s.draft[0], "1999");

        enter_field(&mut s, 0);
        s.handle_key(KeyCode::Esc);
        assert!(s.editing.is_none(), "Esc closes the editor");

        // Esc also invalidates the seed, so the next render snaps back to the
        // live clock and the typed value cannot be applied by accident.
        assert!(!s.seeded, "Esc must invalidate the seeded draft");
        s.seed(&ctx);
        assert_eq!(s.draft[0], "2026", "draft should snap back to the clock");
    }

    #[test]
    fn apply_emits_the_drafted_timestamp() {
        let mut s = screen();
        s.seed(&ViewContext {
            system_time: Some(1_791_924_423),
            ..empty_ctx()
        });
        // 2026-10-13 20:47:03 -> the same instant, so nothing should change.
        s.state.selected = Field::ALL.len();
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::SetTime(1_791_924_423)
        );
    }

    /// Typing a new date by hand must reach the clock, not just validate.
    #[test]
    fn a_hand_typed_date_reaches_the_command() {
        let mut s = screen();
        s.seed(&ViewContext {
            system_time: Some(1_791_924_423),
            ..empty_ctx()
        });

        // Correct the year and the day-of-month by hand.
        retype(&mut s, 0, "2027");
        retype(&mut s, 2, "15");

        s.state.selected = Field::ALL.len();
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::SetTime(timefmt::to_unix(2027, 10, 15, 20, 47, 3).unwrap())
        );
    }

    #[test]
    fn apply_refuses_an_impossible_date() {
        let mut s = screen();
        s.seed(&empty_ctx());
        s.draft = [
            "2026".into(),
            "02".into(),
            "31".into(),
            "00".into(),
            "00".into(),
            "00".into(),
        ];
        s.state.selected = Field::ALL.len();
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::None,
            "31 February must not be applied"
        );
    }

    #[test]
    fn apply_refuses_an_empty_field() {
        let mut s = screen();
        s.seed(&empty_ctx());
        s.draft[2] = String::new();
        s.state.selected = Field::ALL.len();
        assert_eq!(s.handle_key(KeyCode::Enter), Command::None);
    }

    #[test]
    fn apply_refuses_a_pre_2020_date() {
        let mut s = screen();
        s.seed(&empty_ctx());
        s.draft = [
            "1970".into(),
            "01".into(),
            "01".into(),
            "00".into(),
            "00".into(),
            "00".into(),
        ];
        s.state.selected = Field::ALL.len();
        assert_eq!(
            s.handle_key(KeyCode::Enter),
            Command::None,
            "a 1970 clock is exactly what we are trying to escape"
        );
    }

    #[test]
    fn seeding_without_a_clock_starts_near_a_sane_date() {
        let mut s = screen();
        s.seed(&empty_ctx());
        assert_eq!(s.draft[0], "2020");
        // The result must be applicable, so the user can fix the year alone.
        assert!(s.draft_unix().is_some());
    }

    #[test]
    fn navigation_is_bounded_to_the_row_list() {
        let mut s = screen();
        for _ in 0..20 {
            s.handle_key(KeyCode::Down);
        }
        assert_eq!(s.state.selected, Field::ALL.len());

        for _ in 0..20 {
            s.handle_key(KeyCode::Up);
        }
        assert_eq!(s.state.selected, 0);
    }

    #[test]
    fn escape_from_the_list_goes_back() {
        let mut s = screen();
        assert_eq!(s.handle_key(KeyCode::Esc), Command::Back);
    }
}
