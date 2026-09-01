//! Screens of the Reticula UI.

pub mod chat;
pub mod chat_list;
pub mod home;
pub mod new_chat;
pub mod nomad_list;
pub mod nomad_view;
pub mod settings;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use reticula_hal::KeyCode;

use crate::command::{Command, ScreenId};
use crate::context::ViewContext;
use crate::theme::Theme;

use home::HomeScreen;
use chat::ChatScreen;
use chat_list::ChatListScreen;
use new_chat::NewChatScreen;
use nomad_list::NomadListScreen;
use nomad_view::NomadViewScreen;
use settings::SettingsScreen;

/// The active screen. Rendered through the generic display draw target and
/// driven by logical keys.
pub enum Screen {
    Home(HomeScreen),
    ChatList(ChatListScreen),
    Chat(ChatScreen),
    NewChat(NewChatScreen),
    NomadList(NomadListScreen),
    NomadView(NomadViewScreen),
    Settings(SettingsScreen),
}

impl Screen {
    pub fn id(&self) -> ScreenId {
        match self {
            Screen::Home(_) => ScreenId::Home,
            Screen::ChatList(_) => ScreenId::ChatList,
            Screen::Chat(_) => ScreenId::Chat,
            Screen::NewChat(_) => ScreenId::NewChat,
            Screen::NomadList(_) => ScreenId::NomadList,
            Screen::NomadView(_) => ScreenId::NomadView,
            Screen::Settings(_) => ScreenId::Settings,
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Command {
        match self {
            Screen::Home(s) => s.handle_key(key),
            Screen::ChatList(s) => s.handle_key(key),
            Screen::Chat(s) => s.handle_key(key),
            Screen::NewChat(s) => s.handle_key(key),
            Screen::NomadList(s) => s.handle_key(key),
            Screen::NomadView(s) => s.handle_key(key),
            Screen::Settings(s) => s.handle_key(key),
        }
    }

    pub fn render<D>(&mut self, target: &mut D, ctx: &ViewContext, theme: &Theme)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        target.clear(theme.background).ok();
        match self {
            Screen::Home(s) => s.render(target, ctx, theme),
            Screen::ChatList(s) => s.render(target, ctx, theme),
            Screen::Chat(s) => s.render(target, ctx, theme),
            Screen::NewChat(s) => s.render(target, ctx, theme),
            Screen::NomadList(s) => s.render(target, ctx, theme),
            Screen::NomadView(s) => s.render(target, ctx, theme),
            Screen::Settings(s) => s.render(target, ctx, theme),
        }
    }

    /// Whether this screen is currently composing text.
    pub fn is_input(&self) -> bool {
        matches!(self, Screen::Chat(_) | Screen::NewChat(_))
    }
}

/// Scroll/selection state shared by the list-based screens.
#[derive(Debug, Clone, Copy, Default)]
pub struct ListState {
    pub selected: usize,
    pub scroll: usize,
}

impl ListState {
    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self, item_count: usize) {
        if item_count == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(item_count - 1);
    }

    /// Clamp `selected` to `[0, item_count)` without moving it, so a list
    /// that shrank (or grew) does not leave the selection out of range.
    pub fn clamp(&mut self, item_count: usize) {
        if item_count == 0 {
            self.selected = 0;
            self.scroll = 0;
        } else {
            self.selected = self.selected.min(item_count - 1);
        }
    }

    /// Keep `selected` visible within a viewport of `visible` rows.
    pub fn keep_visible(&mut self, visible: usize) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ListState;

    #[test]
    fn clamp_never_moves_selection() {
        let mut s = ListState { selected: 2, scroll: 0 };
        s.clamp(5);
        // Clamping to a larger list must not advance the selection.
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn clamp_bounds_selection_to_shrunk_list() {
        let mut s = ListState { selected: 4, scroll: 3 };
        s.clamp(2);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn clamp_resets_for_empty_list() {
        let mut s = ListState { selected: 3, scroll: 2 };
        s.clamp(0);
        assert_eq!(s.selected, 0);
        assert_eq!(s.scroll, 0);
    }
}