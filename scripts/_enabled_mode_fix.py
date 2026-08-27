from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


hook_path = Path("crates/numflow-windows/src/hook.rs")
hook = hook_path.read_text(encoding="utf-8")
hook = replace_once(
    hook,
    '''    #[must_use]\n    pub fn num_lock_on(&self) -> bool {\n        NUM_LOCK_ON.load(Ordering::Acquire)\n    }\n\n    pub fn set_interception_enabled(&self, enabled: bool) {\n''',
    '''    #[must_use]\n    pub fn num_lock_on(&self) -> bool {\n        NUM_LOCK_ON.load(Ordering::Acquire)\n    }\n\n    /// Synchronizes the tracked and Windows Num Lock state with an explicit runtime request.\n    ///\n    /// A tagged `SendInput` toggle is emitted only when the requested state differs from the\n    /// tracked physical state. `NumFlow` updates interception around that toggle so enabling pointer\n    /// control cannot leak an immediately-following `NumPad` key, while a failed injection restores\n    /// the previous interception state.\n    ///\n    /// Returns `false` when Windows did not accept the complete Num Lock replay sequence.\n    ///\n    /// # Panics\n    ///\n    /// Panics only if compile-time Win32 input structure sizes cannot fit their API integer types.\n    #[must_use]\n    pub fn set_num_lock_on(&self, num_lock_on: bool) -> bool {\n        let current = self.num_lock_on();\n        if current == num_lock_on {\n            self.set_interception_enabled(!num_lock_on);\n            return true;\n        }\n\n        let previous_interception = self.interception_enabled();\n        INTERCEPTION_ENABLED.store(!num_lock_on, Ordering::Release);\n\n        if !replay_num_lock_to_windows() {\n            INTERCEPTION_ENABLED.store(previous_interception, Ordering::Release);\n            return false;\n        }\n\n        NUM_LOCK_ON.store(num_lock_on, Ordering::Release);\n        NUM_LOCK_KEY_DOWN.store(false, Ordering::Release);\n        self.set_interception_enabled(!num_lock_on);\n        true\n    }\n\n    pub fn set_interception_enabled(&self, enabled: bool) {\n''',
    "explicit Num Lock synchronization API",
)
hook_path.write_text(hook, encoding="utf-8")


runtime_path = Path("src/runtime.rs")
runtime = runtime_path.read_text(encoding="utf-8")
runtime = replace_once(
    runtime,
    '''    enum RuntimeCommand {\n        Apply(InputAction),\n        Configure(RuntimeConfig),\n''',
    '''    enum RuntimeCommand {\n        Apply(InputAction),\n        SetEnabled(bool),\n        Configure(RuntimeConfig),\n''',
    "runtime enabled command",
)
runtime = replace_once(
    runtime,
    '''        pub fn apply(&self, action: InputAction) -> Result<(), RuntimeError> {\n            self.send(RuntimeCommand::Apply(action))\n        }\n\n        pub fn configure(&self, config: RuntimeConfig) -> Result<(), RuntimeError> {\n''',
    '''        pub fn apply(&self, action: InputAction) -> Result<(), RuntimeError> {\n            self.send(RuntimeCommand::Apply(action))\n        }\n\n        pub fn set_enabled(&self, enabled: bool) -> Result<(), RuntimeError> {\n            self.send(RuntimeCommand::SetEnabled(enabled))\n        }\n\n        pub fn configure(&self, config: RuntimeConfig) -> Result<(), RuntimeError> {\n''',
    "background runtime enabled API",
)
runtime = replace_once(
    runtime,
    '''        match command {\n            RuntimeCommand::Apply(action) => {\n                if matches!(\n                    action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(());\n                }\n''',
    '''        match command {\n            RuntimeCommand::SetEnabled(enabled) => {\n                normalizer.reset();\n                let num_lock_on = !enabled;\n                if !hook.set_num_lock_on(num_lock_on) {\n                    return Err(format!(\n                        "failed to synchronize Windows Num Lock while setting NumFlow enabled={enabled}"\n                    ));\n                }\n                let _ = apply_num_lock_mode(machine, num_lock_on)\n                    .map_err(|error| error.to_string())?;\n                hook.set_interception_enabled(machine.enabled());\n            }\n            RuntimeCommand::Apply(action) => {\n                if matches!(\n                    action,\n                    InputAction::ToggleEnabled | InputAction::SetEnabled(_)\n                ) {\n                    return Ok(());\n                }\n''',
    "explicit enabled command handling",
)
runtime = replace_once(
    runtime,
    '''    pub fn apply(&self, _action: InputAction) -> Result<(), RuntimeError> {\n        Ok(())\n    }\n\n    pub fn configure(&self, _config: RuntimeConfig) -> Result<(), RuntimeError> {\n''',
    '''    pub fn apply(&self, _action: InputAction) -> Result<(), RuntimeError> {\n        Ok(())\n    }\n\n    pub fn set_enabled(&self, _enabled: bool) -> Result<(), RuntimeError> {\n        Ok(())\n    }\n\n    pub fn configure(&self, _config: RuntimeConfig) -> Result<(), RuntimeError> {\n''',
    "non-Windows enabled API",
)
runtime_path.write_text(runtime, encoding="utf-8")


app_path = Path("src/app.rs")
app = app_path.read_text(encoding="utf-8")
app = replace_once(
    app,
    '''fn runtime_apply(runtime: &SharedRuntime, action: InputAction) {\n    if let Err(error) = runtime.borrow().apply(action) {\n        tracing::error!(%error, ?action, "failed to send action to NumFlow background runtime");\n    }\n}\n\nfn runtime_configure(runtime: &SharedRuntime, settings: &SharedUiSettings) {\n''',
    '''fn runtime_apply(runtime: &SharedRuntime, action: InputAction) {\n    if let Err(error) = runtime.borrow().apply(action) {\n        tracing::error!(%error, ?action, "failed to send action to NumFlow background runtime");\n    }\n}\n\nfn runtime_set_enabled(runtime: &SharedRuntime, enabled: bool) -> bool {\n    match runtime.borrow().set_enabled(enabled) {\n        Ok(()) => true,\n        Err(error) => {\n            tracing::error!(\n                %error,\n                enabled,\n                "failed to send enabled mode to NumFlow background runtime"\n            );\n            false\n        }\n    }\n}\n\nfn runtime_configure(runtime: &SharedRuntime, settings: &SharedUiSettings) {\n''',
    "UI runtime enabled helper",
)
app = replace_once(
    app,
    '''        let runtime = Rc::clone(runtime);\n        let weak_tray = tray.as_weak();\n        window.on_enabled_toggled(move |enabled| {\n            let effects = settings.borrow_mut().set_enabled(enabled);\n            runtime_apply(&runtime, InputAction::SetEnabled(enabled));\n            hud.borrow_mut().observe_effects(&effects);\n            if let Some(tray) = weak_tray.upgrade() {\n                tray.set_numflow_enabled(enabled);\n            }\n        });\n''',
    '''        let runtime = Rc::clone(runtime);\n        let weak_window = window.as_weak();\n        let weak_tray = tray.as_weak();\n        window.on_enabled_toggled(move |enabled| {\n            if !runtime_set_enabled(&runtime, enabled) {\n                let previous = settings.borrow().enabled();\n                if let Some(window) = weak_window.upgrade() {\n                    window.set_numflow_enabled(previous);\n                }\n                if let Some(tray) = weak_tray.upgrade() {\n                    tray.set_numflow_enabled(previous);\n                }\n                return;\n            }\n\n            let effects = settings.borrow_mut().set_enabled(enabled);\n            hud.borrow_mut().observe_effects(&effects);\n            if let Some(tray) = weak_tray.upgrade() {\n                tray.set_numflow_enabled(enabled);\n            }\n        });\n''',
    "settings enabled callback",
)
app = replace_once(
    app,
    '''        let weak_window = window.as_weak();\n        let weak_tray = tray.as_weak();\n        tray.on_enabled_toggled(move |enabled| {\n            let effects = settings.borrow_mut().set_enabled(enabled);\n            runtime_apply(&runtime, InputAction::SetEnabled(enabled));\n            hud.borrow_mut().observe_effects(&effects);\n            if let Some(window) = weak_window.upgrade() {\n                window.set_numflow_enabled(enabled);\n            }\n            if let Some(tray) = weak_tray.upgrade() {\n                tray.set_numflow_enabled(enabled);\n            }\n        });\n''',
    '''        let weak_window = window.as_weak();\n        let weak_tray = tray.as_weak();\n        tray.on_enabled_toggled(move |enabled| {\n            if !runtime_set_enabled(&runtime, enabled) {\n                let previous = settings.borrow().enabled();\n                if let Some(window) = weak_window.upgrade() {\n                    window.set_numflow_enabled(previous);\n                }\n                if let Some(tray) = weak_tray.upgrade() {\n                    tray.set_numflow_enabled(previous);\n                }\n                return;\n            }\n\n            let effects = settings.borrow_mut().set_enabled(enabled);\n            hud.borrow_mut().observe_effects(&effects);\n            if let Some(window) = weak_window.upgrade() {\n                window.set_numflow_enabled(enabled);\n            }\n            if let Some(tray) = weak_tray.upgrade() {\n                tray.set_numflow_enabled(enabled);\n            }\n        });\n''',
    "tray enabled callback",
)
app_path.write_text(app, encoding="utf-8")
