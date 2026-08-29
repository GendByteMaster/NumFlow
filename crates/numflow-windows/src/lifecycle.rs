#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecyclePhase {
    Running,
    Suspending,
    Suspended,
    Resuming,
}

impl LifecyclePhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Suspending => "Suspending",
            Self::Suspended => "Suspended",
            Self::Resuming => "Resuming",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleEventKind {
    PowerSuspend,
    PowerResumeAutomatic,
    PowerResumeUser,
    SessionLock,
    SessionUnlock,
    DesktopInactive,
    DesktopReady,
    ForegroundChanged,
    KeyboardDeviceChanged,
}

impl LifecycleEventKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::PowerSuspend => "PBT_APMSUSPEND",
            Self::PowerResumeAutomatic => "PBT_APMRESUMEAUTOMATIC",
            Self::PowerResumeUser => "PBT_APMRESUMESUSPEND",
            Self::SessionLock => "WTS_SESSION_LOCK",
            Self::SessionUnlock => "WTS_SESSION_UNLOCK",
            Self::DesktopInactive => "EVENT_SYSTEM_DESKTOPSWITCH:inactive",
            Self::DesktopReady => "EVENT_SYSTEM_DESKTOPSWITCH:ready",
            Self::ForegroundChanged => "EVENT_SYSTEM_FOREGROUND",
            Self::KeyboardDeviceChanged => "WM_DEVICECHANGE:keyboard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleEvent {
    pub(crate) kind: LifecycleEventKind,
    pub(crate) token: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleDirective {
    Ignore { reason: &'static str },
    Suspend,
    Quiesce,
    Recover,
    HealthCheck,
    AwaitInteractive { reason: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleTransition {
    pub(crate) event: LifecycleEventKind,
    pub(crate) token: u32,
    pub(crate) from: LifecyclePhase,
    pub(crate) to: LifecyclePhase,
    pub(crate) generation: u32,
    pub(crate) directive: LifecycleDirective,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishedLifecycleState {
    Running,
    Suspending,
    Suspended,
    Resuming,
    SessionLocked,
    DesktopInactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionState {
    Unlocked,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopState {
    Active,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputState {
    Active,
    Quiesced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryState {
    Idle,
    Started,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleMachine {
    phase: LifecyclePhase,
    session: SessionState,
    desktop: DesktopState,
    input: InputState,
    recovery: RecoveryState,
    generation: u32,
    last_event_token: u32,
}

impl Default for LifecycleMachine {
    fn default() -> Self {
        Self {
            phase: LifecyclePhase::Running,
            session: SessionState::Unlocked,
            desktop: DesktopState::Active,
            input: InputState::Active,
            recovery: RecoveryState::Idle,
            generation: 0,
            last_event_token: 0,
        }
    }
}

impl LifecycleMachine {
    pub(crate) fn handle_event(&mut self, event: LifecycleEvent) -> LifecycleTransition {
        let from = self.phase;
        if event.token <= self.last_event_token {
            return self.transition(
                event.kind,
                event.token,
                from,
                LifecycleDirective::Ignore {
                    reason: "stale-generation-token",
                },
            );
        }
        self.last_event_token = event.token;

        let directive = match event.kind {
            LifecycleEventKind::PowerSuspend => self.handle_power_suspend(),
            LifecycleEventKind::PowerResumeAutomatic | LifecycleEventKind::PowerResumeUser => {
                self.handle_power_resume()
            }
            LifecycleEventKind::SessionLock => self.handle_session_lock(),
            LifecycleEventKind::SessionUnlock => self.handle_session_unlock(),
            LifecycleEventKind::DesktopInactive => self.handle_desktop_inactive(),
            LifecycleEventKind::DesktopReady => self.handle_desktop_ready(),
            LifecycleEventKind::ForegroundChanged | LifecycleEventKind::KeyboardDeviceChanged => {
                self.handle_health_checkpoint()
            }
        };

        self.transition(event.kind, event.token, from, directive)
    }

    pub(crate) fn mark_suspended(&mut self, generation: u32) -> bool {
        if self.generation != generation || self.phase != LifecyclePhase::Suspending {
            return false;
        }
        self.phase = LifecyclePhase::Suspended;
        true
    }

    pub(crate) fn complete_recovery(&mut self, generation: u32, succeeded: bool) -> bool {
        if self.generation != generation || self.phase != LifecyclePhase::Resuming {
            return false;
        }

        self.recovery = RecoveryState::Idle;
        if succeeded && self.can_recover() {
            self.input = InputState::Active;
            self.phase = LifecyclePhase::Running;
            return true;
        }
        false
    }

    pub(crate) const fn phase(&self) -> LifecyclePhase {
        self.phase
    }

    pub(crate) const fn generation(&self) -> u32 {
        self.generation
    }

    pub(crate) fn published_state(&self) -> PublishedLifecycleState {
        if self.session == SessionState::Locked {
            return PublishedLifecycleState::SessionLocked;
        }
        match self.phase {
            LifecyclePhase::Suspending => PublishedLifecycleState::Suspending,
            LifecyclePhase::Suspended => PublishedLifecycleState::Suspended,
            LifecyclePhase::Resuming => PublishedLifecycleState::Resuming,
            LifecyclePhase::Running
                if self.desktop == DesktopState::Inactive || self.input == InputState::Quiesced =>
            {
                PublishedLifecycleState::DesktopInactive
            }
            LifecyclePhase::Running => PublishedLifecycleState::Running,
        }
    }

    fn handle_power_suspend(&mut self) -> LifecycleDirective {
        match self.phase {
            LifecyclePhase::Running => {
                self.advance_generation();
                self.phase = LifecyclePhase::Suspending;
                self.input = InputState::Quiesced;
                self.recovery = RecoveryState::Idle;
                LifecycleDirective::Suspend
            }
            LifecyclePhase::Suspending | LifecyclePhase::Suspended => LifecycleDirective::Ignore {
                reason: "duplicate-suspend",
            },
            LifecyclePhase::Resuming => LifecycleDirective::Ignore {
                reason: "stale-suspend-during-recovery",
            },
        }
    }

    fn handle_power_resume(&mut self) -> LifecycleDirective {
        match self.phase {
            LifecyclePhase::Suspending | LifecyclePhase::Suspended => {
                self.advance_generation();
                self.phase = LifecyclePhase::Resuming;
                self.recovery = RecoveryState::Idle;
                self.recover_or_wait()
            }
            LifecyclePhase::Resuming => self.recover_or_wait(),
            LifecyclePhase::Running if self.input == InputState::Quiesced => {
                self.advance_generation();
                self.phase = LifecyclePhase::Resuming;
                self.recover_or_wait()
            }
            LifecyclePhase::Running => LifecycleDirective::Ignore {
                reason: "already-recovered",
            },
        }
    }

    fn handle_session_lock(&mut self) -> LifecycleDirective {
        if self.session == SessionState::Locked {
            return LifecycleDirective::Ignore {
                reason: "duplicate-session-lock",
            };
        }
        let already_quiesced = self.input == InputState::Quiesced;
        self.session = SessionState::Locked;
        self.input = InputState::Quiesced;
        self.recovery = RecoveryState::Idle;
        if already_quiesced {
            LifecycleDirective::Ignore {
                reason: "input-already-quiesced",
            }
        } else {
            LifecycleDirective::Quiesce
        }
    }

    fn handle_session_unlock(&mut self) -> LifecycleDirective {
        if self.session == SessionState::Unlocked && self.input == InputState::Active {
            return LifecycleDirective::Ignore {
                reason: "already-unlocked",
            };
        }
        self.session = SessionState::Unlocked;
        self.start_non_power_recovery_or_wait("desktop-not-ready-after-unlock")
    }

    fn handle_desktop_inactive(&mut self) -> LifecycleDirective {
        if self.desktop == DesktopState::Inactive {
            return LifecycleDirective::Ignore {
                reason: "duplicate-desktop-inactive",
            };
        }
        let already_quiesced = self.input == InputState::Quiesced;
        self.desktop = DesktopState::Inactive;
        self.input = InputState::Quiesced;
        self.recovery = RecoveryState::Idle;
        if already_quiesced {
            LifecycleDirective::Ignore {
                reason: "input-already-quiesced",
            }
        } else {
            LifecycleDirective::Quiesce
        }
    }

    fn handle_desktop_ready(&mut self) -> LifecycleDirective {
        self.desktop = DesktopState::Active;
        if self.input == InputState::Active {
            return LifecycleDirective::Ignore {
                reason: "desktop-already-active",
            };
        }
        self.start_non_power_recovery_or_wait("session-still-locked")
    }

    fn handle_health_checkpoint(&self) -> LifecycleDirective {
        if self.phase == LifecyclePhase::Running
            && self.session == SessionState::Unlocked
            && self.desktop == DesktopState::Active
            && self.input == InputState::Active
        {
            LifecycleDirective::HealthCheck
        } else {
            LifecycleDirective::Ignore {
                reason: "input-lifecycle-not-running",
            }
        }
    }

    fn start_non_power_recovery_or_wait(
        &mut self,
        unavailable_reason: &'static str,
    ) -> LifecycleDirective {
        if self.phase == LifecyclePhase::Suspended || self.phase == LifecyclePhase::Suspending {
            return LifecycleDirective::AwaitInteractive {
                reason: "awaiting-authoritative-power-resume",
            };
        }
        if !self.can_recover() {
            return LifecycleDirective::AwaitInteractive {
                reason: unavailable_reason,
            };
        }
        if self.phase == LifecyclePhase::Running {
            self.advance_generation();
            self.phase = LifecyclePhase::Resuming;
        }
        self.recover_or_wait()
    }

    fn recover_or_wait(&mut self) -> LifecycleDirective {
        if !self.can_recover() {
            return LifecycleDirective::AwaitInteractive {
                reason: "interactive-desktop-unavailable",
            };
        }
        if self.recovery == RecoveryState::Started {
            return LifecycleDirective::Ignore {
                reason: "recovery-already-in-progress",
            };
        }
        self.recovery = RecoveryState::Started;
        LifecycleDirective::Recover
    }

    fn can_recover(&self) -> bool {
        self.session == SessionState::Unlocked && self.desktop == DesktopState::Active
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    const fn transition(
        &self,
        event: LifecycleEventKind,
        token: u32,
        from: LifecyclePhase,
        directive: LifecycleDirective,
    ) -> LifecycleTransition {
        LifecycleTransition {
            event,
            token,
            from,
            to: self.phase,
            generation: self.generation,
            directive,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LifecycleDirective, LifecycleEvent, LifecycleEventKind, LifecycleMachine, LifecyclePhase,
        PublishedLifecycleState,
    };

    fn event(kind: LifecycleEventKind, token: u32) -> LifecycleEvent {
        LifecycleEvent { kind, token }
    }

    fn suspend(machine: &mut LifecycleMachine, token: u32) {
        let transition = machine.handle_event(event(LifecycleEventKind::PowerSuspend, token));
        assert_eq!(transition.directive, LifecycleDirective::Suspend);
        assert!(machine.mark_suspended(transition.generation));
    }

    fn recover(machine: &mut LifecycleMachine, kind: LifecycleEventKind, token: u32) {
        let transition = machine.handle_event(event(kind, token));
        assert_eq!(transition.directive, LifecycleDirective::Recover);
        assert!(machine.complete_recovery(transition.generation, true));
    }

    #[test]
    fn running_suspend_resume_returns_to_running() {
        let mut machine = LifecycleMachine::default();
        suspend(&mut machine, 1);
        assert_eq!(machine.phase(), LifecyclePhase::Suspended);
        recover(&mut machine, LifecycleEventKind::PowerResumeAutomatic, 2);
        assert_eq!(machine.phase(), LifecyclePhase::Running);
        assert_eq!(machine.published_state(), PublishedLifecycleState::Running);
    }

    #[test]
    fn duplicate_suspend_is_idempotent() {
        let mut machine = LifecycleMachine::default();
        suspend(&mut machine, 1);
        let generation = machine.generation();
        let duplicate = machine.handle_event(event(LifecycleEventKind::PowerSuspend, 2));
        assert_eq!(
            duplicate.directive,
            LifecycleDirective::Ignore {
                reason: "duplicate-suspend"
            }
        );
        assert_eq!(machine.generation(), generation);
    }

    #[test]
    fn automatic_user_and_unlock_produce_one_recovery_transaction() {
        let mut machine = LifecycleMachine::default();
        let lock = machine.handle_event(event(LifecycleEventKind::SessionLock, 1));
        assert_eq!(lock.directive, LifecycleDirective::Quiesce);
        suspend(&mut machine, 2);

        let automatic = machine.handle_event(event(LifecycleEventKind::PowerResumeAutomatic, 3));
        assert!(matches!(
            automatic.directive,
            LifecycleDirective::AwaitInteractive { .. }
        ));
        let generation = machine.generation();

        let user = machine.handle_event(event(LifecycleEventKind::PowerResumeUser, 4));
        assert!(matches!(
            user.directive,
            LifecycleDirective::AwaitInteractive { .. }
        ));
        let unlock = machine.handle_event(event(LifecycleEventKind::SessionUnlock, 5));
        assert_eq!(unlock.directive, LifecycleDirective::Recover);
        assert_eq!(unlock.generation, generation);
        assert!(machine.complete_recovery(generation, true));
        assert_eq!(machine.phase(), LifecyclePhase::Running);
    }

    #[test]
    fn stale_suspend_token_cannot_regress_new_generation() {
        let mut machine = LifecycleMachine::default();
        suspend(&mut machine, 10);
        recover(&mut machine, LifecycleEventKind::PowerResumeAutomatic, 12);

        let stale = machine.handle_event(event(LifecycleEventKind::PowerSuspend, 11));
        assert_eq!(
            stale.directive,
            LifecycleDirective::Ignore {
                reason: "stale-generation-token"
            }
        );
        assert_eq!(machine.phase(), LifecyclePhase::Running);
    }

    #[test]
    fn desktop_switch_after_resume_is_not_power_suspend() {
        let mut machine = LifecycleMachine::default();
        suspend(&mut machine, 1);
        recover(&mut machine, LifecycleEventKind::PowerResumeAutomatic, 2);

        let inactive = machine.handle_event(event(LifecycleEventKind::DesktopInactive, 3));
        assert_eq!(inactive.directive, LifecycleDirective::Quiesce);
        assert_eq!(machine.phase(), LifecyclePhase::Running);
        assert_eq!(
            machine.published_state(),
            PublishedLifecycleState::DesktopInactive
        );
        recover(&mut machine, LifecycleEventKind::DesktopReady, 4);
    }

    #[test]
    fn lock_unlock_recovers_without_power_suspend() {
        let mut machine = LifecycleMachine::default();
        assert_eq!(
            machine
                .handle_event(event(LifecycleEventKind::SessionLock, 1))
                .directive,
            LifecycleDirective::Quiesce
        );
        assert_eq!(
            machine.published_state(),
            PublishedLifecycleState::SessionLocked
        );
        recover(&mut machine, LifecycleEventKind::SessionUnlock, 2);
    }

    #[test]
    fn sleep_resume_unlock_waits_for_the_interactive_session() {
        let mut machine = LifecycleMachine::default();
        let _ = machine.handle_event(event(LifecycleEventKind::SessionLock, 1));
        suspend(&mut machine, 2);
        let resume = machine.handle_event(event(LifecycleEventKind::PowerResumeAutomatic, 3));
        assert!(matches!(
            resume.directive,
            LifecycleDirective::AwaitInteractive { .. }
        ));
        recover(&mut machine, LifecycleEventKind::SessionUnlock, 4);
    }

    #[test]
    fn repeated_resume_events_do_not_start_duplicate_recovery() {
        let mut machine = LifecycleMachine::default();
        suspend(&mut machine, 1);
        let automatic = machine.handle_event(event(LifecycleEventKind::PowerResumeAutomatic, 2));
        assert_eq!(automatic.directive, LifecycleDirective::Recover);
        let user = machine.handle_event(event(LifecycleEventKind::PowerResumeUser, 3));
        assert_eq!(
            user.directive,
            LifecycleDirective::Ignore {
                reason: "recovery-already-in-progress"
            }
        );
        assert!(machine.complete_recovery(automatic.generation, true));

        let unlock = machine.handle_event(event(LifecycleEventKind::SessionUnlock, 4));
        assert!(matches!(
            unlock.directive,
            LifecycleDirective::Ignore { .. }
        ));
    }
}
