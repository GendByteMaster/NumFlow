use crate::MouseButton;

/// Platform-independent contract for emitting pointer operations.
///
/// Platform backends implement this trait while application and core code can use mocks in tests.
pub trait PointerBackend {
    type Error;

    /// Moves the pointer by a relative desktop delta.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when the pointer event cannot be emitted completely.
    fn move_relative(&mut self, dx: i32, dy: i32) -> Result<(), Self::Error>;

    /// Presses a mouse button and keeps it held.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when the button-down event cannot be emitted completely.
    fn button_down(&mut self, button: MouseButton) -> Result<(), Self::Error>;

    /// Releases a mouse button previously held by the backend.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when the button-up event cannot be emitted completely.
    fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error>;

    /// Emits one complete click for the requested mouse button.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when the click sequence cannot be emitted completely.
    fn click(&mut self, button: MouseButton) -> Result<(), Self::Error>;

    /// Emits two complete clicks for the requested mouse button.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when the double-click sequence cannot be emitted completely.
    fn double_click(&mut self, button: MouseButton) -> Result<(), Self::Error>;

    /// Releases every mouse button that this backend still considers held.
    ///
    /// # Errors
    ///
    /// Returns a backend-specific error when one or more release events cannot be emitted completely.
    fn release_all(&mut self) -> Result<(), Self::Error>;
}
