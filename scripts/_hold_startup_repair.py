from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"marker not found in {path}: {old[:120]!r}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


# Core: allow an explicit physical button hold without changing the selected click button.
replace_once(
    "crates/numflow-core/src/state.rs",
    """            InputAction::Hold if self.enabled && self.held_button.is_none() => {\n                self.held_button = Some(self.selected_button);\n                vec![CoreEffect::Pointer(PointerEffect::ButtonDown(\n                    self.selected_button,\n                ))]\n            }\n""",
    """            InputAction::Hold => self.hold_button(self.selected_button),\n""",
)
replace_once(
    "crates/numflow-core/src/state.rs",
    """    fn release_held_button(&mut self) -> Vec<CoreEffect> {\n""",
    """    /// Holds a specific physical mouse button without changing the selected click button.\n    ///\n    /// Repeated calls while any button is already held are idempotent and never emit a duplicate\n    /// `ButtonDown`.\n    pub fn hold_button(&mut self, button: MouseButton) -> Vec<CoreEffect> {\n        if !self.enabled || self.held_button.is_some() {\n            return Vec::new();\n        }\n\n        self.held_button = Some(button);\n        vec![CoreEffect::Pointer(PointerEffect::ButtonDown(button))]\n    }\n\n    fn release_held_button(&mut self) -> Vec<CoreEffect> {\n""",
)
replace_once(
    "crates/numflow-core/src/state.rs",
    """    #[test]\n    fn release_without_a_held_button_is_a_no_op() {\n""",
    """    #[test]\n    fn explicit_left_hold_does_not_change_selected_button_and_is_idempotent() {\n        let mut state = ControllerState::default();\n        state.apply(InputAction::SetEnabled(true));\n        state.apply(InputAction::SelectButton(MouseButton::Right));\n\n        assert_eq!(\n            state.hold_button(MouseButton::Left),\n            vec![CoreEffect::Pointer(PointerEffect::ButtonDown(MouseButton::Left))]\n        );\n        assert_eq!(state.selected_button(), MouseButton::Right);\n        assert_eq!(state.held_button(), Some(MouseButton::Left));\n        assert!(state.hold_button(MouseButton::Left).is_empty());\n        assert_eq!(\n            state.apply(InputAction::Release),\n            vec![CoreEffect::Pointer(PointerEffect::ButtonUp(MouseButton::Left))]\n        );\n        assert_eq!(state.held_button(), None);\n    }\n\n    #[test]\n    fn release_without_a_held_button_is_a_no_op() {\n""",
)

# Runtime: make 0/5/+ hold semantics explicit and add keyboard-hook startup retries.
replace_once(
    "src/runtime.rs",
    """        ClickKind, ControllerState, CoreEffect, InputAction, MotionEngine, MotionModifiers,\n        PointerBackend, PointerEffect,\n""",
    """        ClickKind, ControllerState, CoreEffect, InputAction, MotionEngine, MotionModifiers,\n        MouseButton, NumpadKey, PointerBackend, PointerEffect,\n""",
)
replace_once(
    "src/runtime.rs",
    """    const EVENT_QUEUE_CAPACITY: usize = 64;\n""",
    """    const EVENT_QUEUE_CAPACITY: usize = 64;\n    const KEYBOARD_HOOK_START_ATTEMPTS: usize = 3;\n    const KEYBOARD_HOOK_RETRY_DELAY: Duration = Duration::from_millis(100);\n""",
)
replace_once(
    "src/runtime.rs",
    """            if event.state == KeyState::Pressed {\n                if matches!(\n                    event.action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(Vec::new());\n                }\n                self.apply_action(event.action)\n            } else {\n                Ok(Vec::new())\n            }\n""",
    """            if event.state == KeyState::Pressed {\n                // NumPad 0 is the dedicated left-button drag latch. It holds the physical left\n                // button without changing the user's selected click button.\n                if event.key == NumpadKey::Num0 && event.action == InputAction::Hold {\n                    let effects = self.controller.hold_button(MouseButton::Left);\n                    self.execute_effects(&effects)?;\n                    return Ok(effects);\n                }\n\n                // While a drag latch is active, NumPad 5 and + are explicit release controls.\n                // When nothing is held they retain their normal Click / DoubleClick behavior.\n                if matches!(event.key, NumpadKey::Num5 | NumpadKey::Add)\n                    && self.controller.held_button().is_some()\n                {\n                    return self.apply_action(InputAction::Release);\n                }\n\n                if matches!(\n                    event.action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(Vec::new());\n                }\n                self.apply_action(event.action)\n            } else {\n                Ok(Vec::new())\n            }\n""",
)
replace_once(
    "src/runtime.rs",
    """    fn worker_main(\n""",
    """    fn start_keyboard_hook_with_retry(\n    ) -> Result<(KeyboardHook, Receiver<KeyboardHookEvent>), String> {\n        let mut last_error = None;\n\n        for attempt in 1..=KEYBOARD_HOOK_START_ATTEMPTS {\n            match KeyboardHook::start() {\n                Ok(runtime) => {\n                    tracing::info!(attempt, \"NumFlow keyboard hook registered and ready\");\n                    return Ok(runtime);\n                }\n                Err(error) => {\n                    let error = error.to_string();\n                    tracing::warn!(\n                        attempt,\n                        attempts = KEYBOARD_HOOK_START_ATTEMPTS,\n                        %error,\n                        \"failed to initialize NumFlow keyboard hook\"\n                    );\n                    last_error = Some(error);\n                    if attempt < KEYBOARD_HOOK_START_ATTEMPTS {\n                        thread::sleep(KEYBOARD_HOOK_RETRY_DELAY);\n                    }\n                }\n            }\n        }\n\n        Err(last_error.unwrap_or_else(|| \"keyboard hook initialization failed\".to_owned()))\n    }\n\n    fn worker_main(\n""",
)
replace_once(
    "src/runtime.rs",
    """        let (hook, keyboard_receiver) = match KeyboardHook::start() {\n            Ok(runtime) => runtime,\n            Err(error) => {\n                let _ = ready_sender.send(Err(error.to_string()));\n                return;\n            }\n        };\n""",
    """        let (hook, keyboard_receiver) = match start_keyboard_hook_with_retry() {\n            Ok(runtime) => runtime,\n            Err(error) => {\n                let _ = ready_sender.send(Err(error));\n                return;\n            }\n        };\n""",
)
replace_once(
    "src/runtime.rs",
    """            releases: usize,\n""",
    """            releases: usize,\n            clicks: usize,\n            double_clicks: usize,\n""",
)
replace_once(
    "src/runtime.rs",
    """            fn click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {\n                Ok(())\n            }\n\n            fn double_click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {\n                Ok(())\n            }\n""",
    """            fn click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {\n                self.clicks += 1;\n                Ok(())\n            }\n\n            fn double_click(&mut self, _button: MouseButton) -> Result<(), Self::Error> {\n                self.double_clicks += 1;\n                Ok(())\n            }\n""",
)
replace_once(
    "src/runtime.rs",
    """        #[test]\n        fn changing_bindings_stops_existing_motion() {\n""",
    """        fn pressed(key: NumpadKey, action: InputAction) -> NormalizedKeyEvent {\n            NormalizedKeyEvent {\n                key,\n                action,\n                state: KeyState::Pressed,\n                repeated: false,\n            }\n        }\n\n        #[test]\n        fn numpad_zero_holds_left_without_duplicate_mouse_down() {\n            let mut machine = runtime_machine();\n            apply_num_lock_mode(&mut machine, false).expect(\"mock is infallible\");\n            machine\n                .apply_action(InputAction::SelectButton(MouseButton::Right))\n                .expect(\"mock is infallible\");\n\n            let first = machine\n                .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))\n                .expect(\"mock is infallible\");\n            let repeated = machine\n                .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))\n                .expect(\"mock is infallible\");\n\n            assert!(!first.is_empty());\n            assert!(repeated.is_empty());\n            assert_eq!(machine.pointer.held, vec![MouseButton::Left]);\n            assert_eq!(machine.controller.held_button(), Some(MouseButton::Left));\n            assert_eq!(machine.controller.selected_button(), MouseButton::Right);\n        }\n\n        #[test]\n        fn numpad_five_releases_active_hold_and_resets_state() {\n            let mut machine = runtime_machine();\n            apply_num_lock_mode(&mut machine, false).expect(\"mock is infallible\");\n            machine\n                .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))\n                .expect(\"mock is infallible\");\n\n            machine\n                .handle_key_event(pressed(NumpadKey::Num5, InputAction::Click))\n                .expect(\"mock is infallible\");\n\n            assert!(machine.pointer.held.is_empty());\n            assert_eq!(machine.controller.held_button(), None);\n            assert_eq!(machine.pointer.releases, 1);\n            assert_eq!(machine.pointer.clicks, 0);\n\n            machine\n                .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))\n                .expect(\"mock is infallible\");\n            assert_eq!(machine.pointer.held, vec![MouseButton::Left]);\n        }\n\n        #[test]\n        fn numpad_add_releases_active_hold_without_double_clicking() {\n            let mut machine = runtime_machine();\n            apply_num_lock_mode(&mut machine, false).expect(\"mock is infallible\");\n            machine\n                .handle_key_event(pressed(NumpadKey::Num0, InputAction::Hold))\n                .expect(\"mock is infallible\");\n\n            machine\n                .handle_key_event(pressed(NumpadKey::Add, InputAction::DoubleClick))\n                .expect(\"mock is infallible\");\n\n            assert!(machine.pointer.held.is_empty());\n            assert_eq!(machine.controller.held_button(), None);\n            assert_eq!(machine.pointer.releases, 1);\n            assert_eq!(machine.pointer.double_clicks, 0);\n        }\n\n        #[test]\n        fn five_and_add_keep_normal_click_behavior_without_hold() {\n            let mut machine = runtime_machine();\n            apply_num_lock_mode(&mut machine, false).expect(\"mock is infallible\");\n\n            machine\n                .handle_key_event(pressed(NumpadKey::Num5, InputAction::Click))\n                .expect(\"mock is infallible\");\n            machine\n                .handle_key_event(pressed(NumpadKey::Add, InputAction::DoubleClick))\n                .expect(\"mock is infallible\");\n\n            assert_eq!(machine.pointer.clicks, 1);\n            assert_eq!(machine.pointer.double_clicks, 1);\n            assert_eq!(machine.controller.held_button(), None);\n        }\n\n        #[test]\n        fn changing_bindings_stops_existing_motion() {\n""",
)
