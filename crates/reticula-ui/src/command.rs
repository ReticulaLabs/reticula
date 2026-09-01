//! Commands produced by screens for the application to execute.

/// Identifiers of the screens in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreenId {
    Home,
    ChatList,
    Chat,
    NewChat,
    NomadList,
    NomadView,
    Settings,
}

/// An action the UI asks the application to perform.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// No action.
    None,
    /// Switch to another screen (pushing onto the back stack).
    ShowScreen(ScreenId),
    /// Start composing a message to the given peer.
    StartChat([u8; 16]),
    /// Send a message to a peer.
    SendMessage { peer: [u8; 16], text: String },
    /// Fetch a page from a NomadNet node.
    FetchPage { node: [u8; 16], path: String },
    /// Open the fetched page view for a node.
    OpenNode([u8; 16]),
    /// Re-announce our identities.
    Announce,
    /// Set a new display name for our LXMF delivery identity.
    SetDisplayName(String),
    /// Navigate back in the screen stack.
    Back,
    /// Quit the application (host simulator).
    Quit,
}