use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use gradial_core::{
    ActionErrorCode, ActionResult, DEFAULT_ACTION_STEP_TIMEOUT_MS, ExecutionOperation,
    ExecutionPlan, ExecutionStep, InvocationContext, MAX_ACTION_SEQUENCE_DURATION_MS,
    MAX_ACTION_SEQUENCE_STEPS,
};
use uuid::Uuid;

use crate::{platform, tray};

const COMMAND_CAPACITY: usize = 1;
const COMPLETION_CAPACITY: usize = 16;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub(crate) struct CoordinatorStopToken {
    cancellation_requested: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
}

impl CoordinatorStopToken {
    pub(crate) fn cancellation_requested(&self) -> bool {
        self.cancellation_requested.load(Ordering::Acquire)
    }

    pub(crate) fn shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::Acquire)
    }
}

#[derive(Debug)]
pub(crate) struct ContinuousActionLease {
    invocation_id: Uuid,
    busy: Arc<AtomicBool>,
    cancellation_requested: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
}

impl ContinuousActionLease {
    pub(crate) const fn invocation_id(&self) -> Uuid {
        self.invocation_id
    }

    pub(crate) fn stop_token(&self) -> CoordinatorStopToken {
        CoordinatorStopToken {
            cancellation_requested: Arc::clone(&self.cancellation_requested),
            shutdown_requested: Arc::clone(&self.shutdown_requested),
        }
    }
}

impl Drop for ContinuousActionLease {
    fn drop(&mut self) {
        tray::set_action_running(false);
        self.busy.store(false, Ordering::Release);
        platform::request_controller_repaint();
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ActionExecutorHandle {
    commands: Sender<WorkerCommand>,
    busy: Arc<AtomicBool>,
    cancellation_requested: Arc<AtomicBool>,
    shutdown_requested: Arc<AtomicBool>,
    worker_thread: Thread,
}

impl ActionExecutorHandle {
    pub(crate) fn try_dispatch(
        &self,
        invocation_id: Uuid,
        plan: Arc<ExecutionPlan>,
        target: Arc<InvocationContext>,
    ) -> ActionDispatch {
        if let Err(result) = self.try_claim_coordinator() {
            return ActionDispatch::Rejected(result);
        }

        let request = ExecutionRequest { invocation_id, plan, target };
        match self.commands.try_send(WorkerCommand::Execute(request)) {
            Ok(()) => {
                self.worker_thread.unpark();
                ActionDispatch::Accepted
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                tray::set_action_running(false);
                self.busy.store(false, Ordering::Release);
                ActionDispatch::Rejected(failure(
                    ActionErrorCode::ExecutorUnavailable,
                    "the action executor is unavailable",
                ))
            }
        }
    }

    pub(crate) fn try_acquire_continuous(
        &self,
        invocation_id: Uuid,
    ) -> Result<ContinuousActionLease, ActionResult> {
        self.try_claim_coordinator()?;
        Ok(ContinuousActionLease {
            invocation_id,
            busy: Arc::clone(&self.busy),
            cancellation_requested: Arc::clone(&self.cancellation_requested),
            shutdown_requested: Arc::clone(&self.shutdown_requested),
        })
    }

    pub(crate) fn cancel_current(&self) -> bool {
        if !self.busy.load(Ordering::Acquire) {
            return false;
        }
        self.cancellation_requested.store(true, Ordering::Release);
        self.worker_thread.unpark();
        true
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    fn try_claim_coordinator(&self) -> Result<(), ActionResult> {
        if self.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(failure(ActionErrorCode::ActionBusy, "another action is already running"));
        }
        if self.shutdown_requested.load(Ordering::Acquire) {
            self.busy.store(false, Ordering::Release);
            return Err(failure(
                ActionErrorCode::ExecutorUnavailable,
                "the action executor is shutting down",
            ));
        }
        self.cancellation_requested.store(false, Ordering::Release);
        tray::set_action_running(true);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ActionDispatch {
    Accepted,
    Rejected(ActionResult),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActionCompletion {
    pub(crate) invocation_id: Uuid,
    pub(crate) root_action_id: Uuid,
    pub(crate) config_revision: u64,
    pub(crate) step_count: usize,
    pub(crate) elapsed_ms: u128,
    pub(crate) result: ActionResult,
}

#[derive(Debug)]
pub(crate) struct ActionExecutor {
    handle: ActionExecutorHandle,
    completions: Receiver<ActionCompletion>,
    stopped: Receiver<()>,
    join_handle: Option<JoinHandle<()>>,
    shutdown_wait: Duration,
    config_revision: u64,
}

impl ActionExecutor {
    pub(crate) fn start(config_revision: u64) -> Result<Self> {
        Self::start_with_policy(config_revision, ExecutionPolicy::production())
    }

    fn start_with_policy(config_revision: u64, policy: ExecutionPolicy) -> Result<Self> {
        let (command_sender, commands) = bounded(COMMAND_CAPACITY);
        let (completion_sender, completions) = bounded(COMPLETION_CAPACITY);
        let (stopped_sender, stopped) = bounded(1);
        let busy = Arc::new(AtomicBool::new(false));
        let cancellation_requested = Arc::new(AtomicBool::new(false));
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let worker_busy = Arc::clone(&busy);
        let worker_cancellation = Arc::clone(&cancellation_requested);
        let worker_shutdown = Arc::clone(&shutdown_requested);

        let join_handle = thread::Builder::new()
            .name("gradial-action-executor".to_owned())
            .spawn(move || {
                run_worker(
                    &commands,
                    &completion_sender,
                    &worker_busy,
                    &worker_cancellation,
                    &worker_shutdown,
                    policy,
                );
                let _sent = stopped_sender.send(());
            })
            .context("could not spawn the action executor")?;
        let worker_thread = join_handle.thread().clone();
        let handle = ActionExecutorHandle {
            commands: command_sender,
            busy,
            cancellation_requested,
            shutdown_requested,
            worker_thread,
        };
        tray::set_action_running(false);
        log::info!(
            target: "actions",
            "component=actions event_code=action_executor_ready result_code=ok \
             config_revision={config_revision} \
             supported_operations=delay,send_shortcut,type_text,open_url,open_path,run_executable,window \
             per_step_timeout_ms={DEFAULT_ACTION_STEP_TIMEOUT_MS} \
             maximum_duration_ms={MAX_ACTION_SEQUENCE_DURATION_MS} \
             maximum_steps={MAX_ACTION_SEQUENCE_STEPS} cancellation_poll_ms={} \
             shutdown_wait_ms={}",
            CANCELLATION_POLL_INTERVAL.as_millis(),
            policy.shutdown_wait.as_millis()
        );

        Ok(Self {
            handle,
            completions,
            stopped,
            join_handle: Some(join_handle),
            shutdown_wait: policy.shutdown_wait,
            config_revision,
        })
    }

    pub(crate) fn handle(&self) -> ActionExecutorHandle {
        self.handle.clone()
    }

    pub(crate) fn cancel_current(&self) -> bool {
        self.handle.cancel_current()
    }

    pub(crate) fn try_completion(&self) -> Option<ActionCompletion> {
        self.completions.try_recv().ok()
    }

    pub(crate) fn set_config_revision(&mut self, config_revision: u64) {
        self.config_revision = config_revision;
    }

    pub(crate) fn shutdown(&mut self) {
        let Some(join_handle) = self.join_handle.take() else {
            return;
        };
        let action_was_running = self.handle.is_busy();
        let shutdown_started = Instant::now();
        self.handle.shutdown_requested.store(true, Ordering::Release);
        self.handle.cancellation_requested.store(true, Ordering::Release);
        self.handle.worker_thread.unpark();
        let _wake = self.handle.commands.try_send(WorkerCommand::Shutdown);
        log::info!(
            target: "actions",
            "component=actions event_code=action_executor_shutdown_requested result_code=ok \
             config_revision={} action_running={} wait_ms={}",
            self.config_revision,
            action_was_running,
            self.shutdown_wait.as_millis()
        );

        match self.stopped.recv_timeout(self.shutdown_wait) {
            Ok(()) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                if join_handle.join().is_err() {
                    log::error!(
                        target: "actions",
                        "component=actions event_code=action_executor_panicked \
                         result_code=failure config_revision={}",
                        self.config_revision
                    );
                } else {
                    log::info!(
                        target: "actions",
                        "component=actions event_code=action_executor_stopped result_code=ok \
                         config_revision={}",
                        self.config_revision
                    );
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                log::error!(
                    target: "actions",
                    "component=actions event_code=action_executor_shutdown_timeout \
                     result_code=failure error_code=shutdown_timeout config_revision={} \
                     wait_ms={}",
                    self.config_revision,
                    self.shutdown_wait.as_millis()
                );
            }
        }
        while self.handle.is_busy() && shutdown_started.elapsed() < self.shutdown_wait {
            thread::sleep(CANCELLATION_POLL_INTERVAL);
        }
        if self.handle.is_busy() {
            log::error!(
                target: "actions",
                "component=actions event_code=action_coordinator_shutdown_timeout \
                 result_code=failure error_code=shutdown_timeout config_revision={} \
                 wait_ms={}",
                self.config_revision,
                self.shutdown_wait.as_millis()
            );
        }
        tray::set_action_running(false);
        self.handle.busy.store(false, Ordering::Release);
    }
}

impl Drop for ActionExecutor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug)]
enum WorkerCommand {
    Execute(ExecutionRequest),
    Shutdown,
}

#[derive(Debug)]
struct ExecutionRequest {
    invocation_id: Uuid,
    plan: Arc<ExecutionPlan>,
    target: Arc<InvocationContext>,
}

#[derive(Clone, Copy, Debug)]
struct ExecutionPolicy {
    cancellation_poll_interval: Duration,
    per_step_timeout_override: Option<Duration>,
    maximum_duration_override: Option<Duration>,
    shutdown_wait: Duration,
}

impl ExecutionPolicy {
    const fn production() -> Self {
        Self {
            cancellation_poll_interval: CANCELLATION_POLL_INTERVAL,
            per_step_timeout_override: None,
            maximum_duration_override: None,
            shutdown_wait: SHUTDOWN_WAIT,
        }
    }
}

fn run_worker(
    commands: &Receiver<WorkerCommand>,
    completions: &Sender<ActionCompletion>,
    busy: &AtomicBool,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    policy: ExecutionPolicy,
) {
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Execute(request) => {
                let started = Instant::now();
                log::info!(
                    target: "actions",
                    "component=actions event_code=action_execution_started result_code=ok \
                     config_revision={} invocation_id={} action_id={} step_count={}",
                    request.plan.config_revision(),
                    request.invocation_id,
                    request.plan.root_action_id(),
                    request.plan.steps().len()
                );
                let result = execute_plan(
                    request.invocation_id,
                    &request.plan,
                    &request.target,
                    cancellation_requested,
                    shutdown_requested,
                    policy,
                );
                let completion = ActionCompletion {
                    invocation_id: request.invocation_id,
                    root_action_id: request.plan.root_action_id(),
                    config_revision: request.plan.config_revision(),
                    step_count: request.plan.steps().len(),
                    elapsed_ms: started.elapsed().as_millis(),
                    result,
                };
                log_completion(&completion);
                let completion_config_revision = completion.config_revision;
                let completion_invocation_id = completion.invocation_id;
                if let Err(error) = completions.try_send(completion) {
                    let reason = if matches!(error, TrySendError::Full(_)) {
                        "queue_full"
                    } else {
                        "closed"
                    };
                    log::warn!(
                        target: "actions",
                        "component=actions event_code=action_completion_delivery_failed \
                         result_code=failure error_code={reason} \
                         config_revision={completion_config_revision} \
                         invocation_id={completion_invocation_id}"
                    );
                }
                tray::set_action_running(false);
                busy.store(false, Ordering::Release);
                platform::request_controller_repaint();
                if shutdown_requested.load(Ordering::Acquire) {
                    break;
                }
            }
            WorkerCommand::Shutdown => break,
        }
    }
    tray::set_action_running(false);
    busy.store(false, Ordering::Release);
    platform::request_controller_repaint();
}

fn execute_plan(
    invocation_id: Uuid,
    plan: &ExecutionPlan,
    target: &InvocationContext,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    policy: ExecutionPolicy,
) -> ActionResult {
    if plan.steps().len() > MAX_ACTION_SEQUENCE_STEPS {
        return failure(
            ActionErrorCode::StepLimitExceeded,
            "the action exceeds the executor step limit",
        );
    }
    let plan_started = Instant::now();
    let maximum_duration = policy
        .maximum_duration_override
        .unwrap_or_else(|| Duration::from_millis(u64::from(plan.maximum_duration_ms())));
    for step in plan.steps() {
        if cancellation_requested.load(Ordering::Acquire)
            || shutdown_requested.load(Ordering::Acquire)
        {
            return ActionResult::Cancelled;
        }
        let per_step_timeout = policy
            .per_step_timeout_override
            .unwrap_or_else(|| Duration::from_millis(u64::from(plan.per_step_timeout_ms())));
        let step_started = Instant::now();
        let check_deadline_after_step = matches!(
            step.operation(),
            ExecutionOperation::SendShortcut { .. }
                | ExecutionOperation::TypeText { .. }
                | ExecutionOperation::OpenUrl { .. }
                | ExecutionOperation::OpenPath { .. }
                | ExecutionOperation::RunExecutable { .. }
                | ExecutionOperation::Window { .. }
        );
        let result = execute_step(
            step,
            StepRuntime {
                invocation_id,
                config_revision: plan.config_revision(),
                target,
                cancellation_requested,
                shutdown_requested,
                plan_started,
                maximum_duration,
                step_started,
                per_step_timeout,
                cancellation_poll_interval: policy.cancellation_poll_interval,
            },
        );
        if let Some(result) = result
            && result != ActionResult::Success
        {
            return result;
        }
        if check_deadline_after_step && step_started.elapsed() >= per_step_timeout {
            return failure(ActionErrorCode::StepTimeout, "an action step timed out");
        }
        if check_deadline_after_step && plan_started.elapsed() >= maximum_duration {
            return failure(
                ActionErrorCode::ActionTimeout,
                "the action exceeded its maximum duration",
            );
        }
    }
    ActionResult::Success
}

#[derive(Clone, Copy, Debug)]
struct StepRuntime<'a> {
    invocation_id: Uuid,
    config_revision: u64,
    target: &'a InvocationContext,
    cancellation_requested: &'a AtomicBool,
    shutdown_requested: &'a AtomicBool,
    plan_started: Instant,
    maximum_duration: Duration,
    step_started: Instant,
    per_step_timeout: Duration,
    cancellation_poll_interval: Duration,
}

fn execute_step(step: &ExecutionStep, runtime: StepRuntime<'_>) -> Option<ActionResult> {
    match step.operation() {
        ExecutionOperation::Delay { milliseconds } => execute_delay(
            Duration::from_millis(u64::from(*milliseconds)),
            runtime.plan_started,
            runtime.maximum_duration,
            runtime.per_step_timeout,
            runtime.cancellation_requested,
            runtime.shutdown_requested,
            runtime.cancellation_poll_interval,
        ),
        ExecutionOperation::SendShortcut { modifiers, key } => Some(crate::shortcut::execute(
            runtime.target,
            modifiers,
            *key,
            runtime.cancellation_requested,
            runtime.shutdown_requested,
        )),
        ExecutionOperation::TypeText { text } => {
            log::info!(
                target: "actions",
                "component=actions event_code=text_step_started result_code=ok \
                 config_revision={} invocation_id={} action_id={} character_count={}",
                runtime.config_revision,
                runtime.invocation_id,
                step.source_action_id(),
                text.chars().count()
            );
            Some(crate::unicode_text::execute(
                runtime.target,
                text,
                runtime.cancellation_requested,
                runtime.shutdown_requested,
                crate::unicode_text::TextDeadlines::new(
                    runtime.step_started,
                    runtime.per_step_timeout,
                    runtime.plan_started,
                    runtime.maximum_duration,
                ),
            ))
        }
        operation @ (ExecutionOperation::OpenUrl { .. } | ExecutionOperation::OpenPath { .. }) => {
            Some(execute_shell_open_step(
                operation,
                crate::shell_open::OpenLogContext::new(
                    runtime.config_revision,
                    runtime.invocation_id,
                    step.source_action_id(),
                ),
                runtime.cancellation_requested,
                runtime.shutdown_requested,
                crate::shell_open::OpenDeadlines::new(
                    runtime.step_started,
                    runtime.per_step_timeout,
                    runtime.plan_started,
                    runtime.maximum_duration,
                ),
            ))
        }
        ExecutionOperation::RunExecutable { executable, arguments, working_directory } => {
            Some(crate::process_launch::execute(
                executable,
                arguments,
                working_directory.as_deref(),
                runtime.cancellation_requested,
                runtime.shutdown_requested,
                crate::process_launch::LaunchDeadlines::new(
                    runtime.step_started,
                    runtime.per_step_timeout,
                    runtime.plan_started,
                    runtime.maximum_duration,
                ),
                crate::process_launch::LaunchLogContext::new(
                    runtime.config_revision,
                    runtime.invocation_id,
                    step.source_action_id(),
                ),
            ))
        }
        ExecutionOperation::Window { command } => Some(crate::window_action::execute(
            runtime.target,
            *command,
            runtime.cancellation_requested,
            runtime.shutdown_requested,
            crate::window_action::WindowDeadlines::new(
                runtime.step_started,
                runtime.per_step_timeout,
                runtime.plan_started,
                runtime.maximum_duration,
            ),
            crate::window_action::WindowLogContext::new(
                runtime.config_revision,
                runtime.invocation_id,
                step.source_action_id(),
            ),
        )),
    }
}

fn execute_shell_open_step(
    operation: &ExecutionOperation,
    log_context: crate::shell_open::OpenLogContext,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    deadlines: crate::shell_open::OpenDeadlines,
) -> ActionResult {
    match operation {
        ExecutionOperation::OpenUrl { url } => crate::shell_open::open_url(
            url,
            cancellation_requested,
            shutdown_requested,
            deadlines,
            log_context,
        ),
        ExecutionOperation::OpenPath { path } => crate::shell_open::open_path(
            path,
            cancellation_requested,
            shutdown_requested,
            deadlines,
            log_context,
        ),
        _ => unreachable!("the caller accepts only shell-open operations"),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_delay(
    duration: Duration,
    plan_started: Instant,
    maximum_duration: Duration,
    per_step_timeout: Duration,
    cancellation_requested: &AtomicBool,
    shutdown_requested: &AtomicBool,
    cancellation_poll_interval: Duration,
) -> Option<ActionResult> {
    let step_started = Instant::now();
    loop {
        if cancellation_requested.load(Ordering::Acquire)
            || shutdown_requested.load(Ordering::Acquire)
        {
            return Some(ActionResult::Cancelled);
        }
        let now = Instant::now();
        if now.duration_since(plan_started) >= maximum_duration {
            return Some(failure(
                ActionErrorCode::ActionTimeout,
                "the action exceeded its maximum duration",
            ));
        }
        if now.duration_since(step_started) >= duration {
            return None;
        }
        if now.duration_since(step_started) >= per_step_timeout {
            return Some(failure(ActionErrorCode::StepTimeout, "an action step timed out"));
        }

        let delay_remaining = duration.saturating_sub(now.duration_since(step_started));
        let step_remaining = per_step_timeout.saturating_sub(now.duration_since(step_started));
        let plan_remaining = maximum_duration.saturating_sub(now.duration_since(plan_started));
        thread::park_timeout(
            cancellation_poll_interval.min(delay_remaining).min(step_remaining).min(plan_remaining),
        );
    }
}

fn failure(code: ActionErrorCode, safe_message: &str) -> ActionResult {
    ActionResult::Failure { code, safe_message: safe_message.to_owned() }
}

fn log_completion(completion: &ActionCompletion) {
    match &completion.result {
        ActionResult::Success => {
            log::info!(
                target: "actions",
                "component=actions event_code=action_execution_completed result_code=ok \
                 config_revision={} invocation_id={} action_id={} step_count={} \
                 elapsed_ms={} action_executed=true",
                completion.config_revision,
                completion.invocation_id,
                completion.root_action_id,
                completion.step_count,
                completion.elapsed_ms
            );
        }
        ActionResult::Cancelled => {
            log::info!(
                target: "actions",
                "component=actions event_code=action_execution_completed result_code=cancelled \
                 config_revision={} invocation_id={} action_id={} step_count={} \
                 elapsed_ms={} action_executed=false",
                completion.config_revision,
                completion.invocation_id,
                completion.root_action_id,
                completion.step_count,
                completion.elapsed_ms
            );
        }
        ActionResult::Failure { code, .. } => {
            log::error!(
                target: "actions",
                "component=actions event_code=action_execution_completed result_code=failure \
                 error_code={} config_revision={} invocation_id={} action_id={} \
                 step_count={} elapsed_ms={} action_executed=false",
                code.as_str(),
                completion.config_revision,
                completion.invocation_id,
                completion.root_action_id,
                completion.step_count,
                completion.elapsed_ms
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, thread, time::Duration};

    use gradial_core::{
        ActionDefinition, ActionErrorCode, ActionKind, ActionPlanCompiler, ActionResult,
        Configuration,
    };
    use uuid::Uuid;

    use super::{ActionDispatch, ActionExecutor, ActionExecutorHandle, ExecutionPolicy};

    fn delay_plan(milliseconds: u32, revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Delay".to_owned(),
                kind: ActionKind::Delay { milliseconds },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), revision).expect("plan"))
    }

    fn shortcut_plan(revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Shortcut".to_owned(),
                kind: ActionKind::SendShortcut {
                    modifiers: vec![gradial_core::ShortcutModifier::LeftControl],
                    key: gradial_core::ShortcutKey { scan_code: 0x64, extended: false },
                },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), revision).expect("plan"))
    }

    fn text_plan(revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Text".to_owned(),
                kind: ActionKind::TypeText { text: "private text".to_owned() },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), revision).expect("plan"))
    }

    fn window_plan(revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Window".to_owned(),
                kind: ActionKind::Window { command: gradial_core::WindowCommand::Minimize },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), revision).expect("plan"))
    }

    fn open_path_sequence_plan(path: PathBuf, revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![
                ActionDefinition {
                    id: Uuid::from_u128(1),
                    name: "Delay".to_owned(),
                    kind: ActionKind::Delay { milliseconds: 10 },
                },
                ActionDefinition {
                    id: Uuid::from_u128(2),
                    name: "Path".to_owned(),
                    kind: ActionKind::OpenPath { path },
                },
                ActionDefinition {
                    id: Uuid::from_u128(3),
                    name: "Sequence".to_owned(),
                    kind: ActionKind::Sequence {
                        actions: vec![Uuid::from_u128(1), Uuid::from_u128(2)],
                    },
                },
            ],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(3), revision).expect("plan"))
    }

    fn executable_plan(executable: PathBuf, revision: u64) -> Arc<gradial_core::ExecutionPlan> {
        let configuration = Configuration {
            config_revision: revision,
            actions: vec![ActionDefinition {
                id: Uuid::from_u128(1),
                name: "Executable".to_owned(),
                kind: ActionKind::RunExecutable {
                    executable,
                    arguments: vec!["private argument".to_owned()],
                    working_directory: None,
                },
            }],
            ..Configuration::default()
        };
        let compiler = ActionPlanCompiler::from_configuration(&configuration).expect("compiler");
        Arc::new(compiler.compile_plan(Uuid::from_u128(1), revision).expect("plan"))
    }

    fn wait_for_completion(executor: &ActionExecutor) -> super::ActionCompletion {
        for _ in 0..200 {
            if let Some(completion) = executor.try_completion() {
                return completion;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("action did not complete");
    }

    fn dispatch(
        handle: &ActionExecutorHandle,
        invocation_id: Uuid,
        plan: Arc<gradial_core::ExecutionPlan>,
    ) -> ActionDispatch {
        handle.try_dispatch(
            invocation_id,
            plan,
            Arc::new(gradial_core::InvocationContext::default()),
        )
    }

    #[test]
    fn only_one_plan_is_admitted_and_delay_completes() {
        let executor = ActionExecutor::start(1).expect("executor");
        let handle = executor.handle();
        let plan = delay_plan(20, 1);

        assert_eq!(
            dispatch(&handle, Uuid::from_u128(11), Arc::clone(&plan)),
            ActionDispatch::Accepted
        );
        let rejected = dispatch(&handle, Uuid::from_u128(12), Arc::clone(&plan));
        assert!(matches!(
            rejected,
            ActionDispatch::Rejected(ActionResult::Failure {
                code: ActionErrorCode::ActionBusy,
                ..
            })
        ));

        let completion = wait_for_completion(&executor);
        assert_eq!(completion.invocation_id, Uuid::from_u128(11));
        assert_eq!(completion.result, ActionResult::Success);
        assert!(!handle.is_busy());

        assert_eq!(dispatch(&handle, Uuid::from_u128(13), plan), ActionDispatch::Accepted);
        assert_eq!(wait_for_completion(&executor).result, ActionResult::Success);
    }

    #[test]
    fn cancellation_interrupts_a_delay_without_waiting_for_its_deadline() {
        let executor = ActionExecutor::start(1).expect("executor");
        let handle = executor.handle();
        let plan = delay_plan(500, 1);

        assert_eq!(dispatch(&handle, Uuid::from_u128(21), plan), ActionDispatch::Accepted);
        thread::sleep(Duration::from_millis(20));
        assert!(handle.cancel_current());

        let completion = wait_for_completion(&executor);
        assert_eq!(completion.result, ActionResult::Cancelled);
        assert!(completion.elapsed_ms < 250);

        assert_eq!(
            dispatch(&handle, Uuid::from_u128(22), delay_plan(20, 1)),
            ActionDispatch::Accepted
        );
        assert_eq!(wait_for_completion(&executor).result, ActionResult::Success);
    }

    #[test]
    fn per_step_and_overall_timeouts_are_enforced() {
        let step_policy = ExecutionPolicy {
            per_step_timeout_override: Some(Duration::from_millis(15)),
            ..ExecutionPolicy::production()
        };
        let step_executor = ActionExecutor::start_with_policy(1, step_policy).expect("executor");
        assert_eq!(
            dispatch(&step_executor.handle(), Uuid::from_u128(31), delay_plan(100, 1)),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&step_executor).result,
            ActionResult::Failure { code: ActionErrorCode::StepTimeout, .. }
        ));

        let overall_policy = ExecutionPolicy {
            per_step_timeout_override: Some(Duration::from_secs(1)),
            maximum_duration_override: Some(Duration::from_millis(15)),
            ..ExecutionPolicy::production()
        };
        let overall_executor =
            ActionExecutor::start_with_policy(1, overall_policy).expect("executor");
        assert_eq!(
            dispatch(&overall_executor.handle(), Uuid::from_u128(32), delay_plan(100, 1)),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&overall_executor).result,
            ActionResult::Failure { code: ActionErrorCode::ActionTimeout, .. }
        ));
    }

    #[test]
    fn shortcut_reaches_target_validation_before_any_input_is_sent() {
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(&executor.handle(), Uuid::from_u128(35), shortcut_plan(1)),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&executor).result,
            ActionResult::Failure { code: ActionErrorCode::StaleTarget, .. }
        ));
    }

    #[test]
    fn text_reaches_target_validation_before_any_input_is_sent() {
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(&executor.handle(), Uuid::from_u128(36), text_plan(1)),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&executor).result,
            ActionResult::Failure { code: ActionErrorCode::StaleTarget, .. }
        ));
    }

    #[test]
    fn window_command_reaches_target_validation_before_dispatch() {
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(&executor.handle(), Uuid::from_u128(39), window_plan(1)),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&executor).result,
            ActionResult::Failure { code: ActionErrorCode::StaleTarget, .. }
        ));
    }

    #[test]
    fn enabled_sequence_reaches_missing_path_without_shell_dispatch() {
        let missing_path =
            std::env::temp_dir().join(format!("gradial-missing-{}.txt", Uuid::new_v4()));
        assert!(!missing_path.exists());
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(
                &executor.handle(),
                Uuid::from_u128(37),
                open_path_sequence_plan(missing_path, 1),
            ),
            ActionDispatch::Accepted
        );
        let completion = wait_for_completion(&executor);
        assert_eq!(completion.step_count, 2);
        assert!(matches!(
            completion.result,
            ActionResult::Failure { code: ActionErrorCode::PathNotFound, .. }
        ));
    }

    #[test]
    fn executable_plan_reaches_missing_path_without_process_creation() {
        let missing_executable =
            std::env::temp_dir().join(format!("gradial-missing-{}.exe", Uuid::new_v4()));
        assert!(!missing_executable.exists());
        let executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(
                &executor.handle(),
                Uuid::from_u128(38),
                executable_plan(missing_executable, 1),
            ),
            ActionDispatch::Accepted
        );
        assert!(matches!(
            wait_for_completion(&executor).result,
            ActionResult::Failure { code: ActionErrorCode::PathNotFound, .. }
        ));
    }

    #[test]
    fn shutdown_cancels_an_active_delay_within_its_bound() {
        let started = std::time::Instant::now();
        let mut executor = ActionExecutor::start(1).expect("executor");
        assert_eq!(
            dispatch(&executor.handle(), Uuid::from_u128(51), delay_plan(1_000, 1)),
            ActionDispatch::Accepted
        );
        thread::sleep(Duration::from_millis(20));

        executor.shutdown();

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!executor.handle().is_busy());
    }
}
