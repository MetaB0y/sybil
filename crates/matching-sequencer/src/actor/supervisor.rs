use super::*;
use ractor::SupervisionEvent;

#[derive(Clone)]
pub(super) struct SequencerHandleInner {
    pub(super) actor: Arc<RwLock<Option<ActorRef<SequencerMsg>>>>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) recent_blocks: Arc<RwLock<VecDeque<SealedBlock>>>,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) mailbox_monitor: MailboxMonitor,
    pub(super) rpc_admission: RpcAdmission,
    pub(super) shutdown_requested: Arc<AtomicBool>,
    pub(super) fatal_error: watch::Sender<Option<String>>,
}

impl SequencerHandleInner {
    pub(super) fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        self.mailbox_monitor.reset();
        *self
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = actor;
    }

    fn report_terminal_failure(&self, stage: &'static str, error: impl std::fmt::Display) {
        if self.shutdown_requested.load(Ordering::Acquire) || self.fatal_error.borrow().is_some() {
            return;
        }
        let message = format!("{stage}: {error}");
        metrics::counter!(
            "sybil_sequencer_terminal_failures_total",
            "stage" => stage
        )
        .increment(1);
        tracing::error!(stage, error = %error, "canonical sequencer ownership was lost");
        self.fatal_error.send_replace(Some(message));
    }
}

pub(super) struct SequencerSupervisor;

pub(super) struct SequencerSupervisorArgs {
    pub(super) config: SequencerConfig,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) handle: SequencerHandleInner,
}

pub(super) struct SequencerSupervisorState {
    current_actor: Option<ActorRef<SequencerMsg>>,
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    handle: SequencerHandleInner,
}

pub(super) enum SequencerSupervisorMsg {
    AdoptChild(ActorRef<SequencerMsg>),
}

impl SequencerSupervisorState {
    fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        self.handle.publish_actor(actor);
    }

    async fn spawn_child(
        &mut self,
        myself: ActorRef<SequencerSupervisorMsg>,
        sequencer: BlockSequencer,
    ) -> Result<(), ActorProcessingErr> {
        let args = SequencerActorArgs {
            sequencer,
            store: self.store.clone(),
            block_broadcast: self.handle.block_broadcast.clone(),
            recent_blocks: self.handle.recent_blocks.clone(),
            mailbox_monitor: self.handle.mailbox_monitor.clone(),
        };
        let (child, _) =
            <SequencerActor as Actor>::spawn_linked(None, SequencerActor, args, myself.get_cell())
                .await
                .map_err(|error| ActorProcessingErr::from(error.to_string()))?;
        self.current_actor = Some(child.clone());
        self.publish_actor(Some(child));
        Ok(())
    }

    async fn restart_from_store(&mut self, myself: ActorRef<SequencerSupervisorMsg>) {
        self.current_actor = None;
        self.publish_actor(None);

        if self.handle.shutdown_requested.load(Ordering::Acquire) {
            return;
        }

        let Some(store) = self.store.clone() else {
            self.handle.report_terminal_failure(
                "store_unavailable",
                "sequencer actor exited without a persistent store",
            );
            return;
        };

        let restored = match store.load_state().await {
            Ok(state) => state,
            Err(error) => {
                self.handle.report_terminal_failure("load_state", error);
                return;
            }
        };

        let Some(state) = restored else {
            self.handle
                .report_terminal_failure("snapshot_missing", "no persisted sequencer snapshot");
            return;
        };

        if self.handle.shutdown_requested.load(Ordering::Acquire) {
            return;
        }

        let sequencer = match BlockSequencer::try_restore(state, self.config.clone()) {
            Ok(sequencer) => sequencer,
            Err(error) => {
                self.handle.report_terminal_failure("restore", error);
                return;
            }
        };

        match self.spawn_child(myself, sequencer).await {
            Ok(()) => tracing::warn!("sequencer actor restarted from persistent snapshot"),
            Err(error) => {
                self.handle.report_terminal_failure("spawn_child", error);
            }
        }
    }
}

#[ractor::async_trait]
impl Actor for SequencerSupervisor {
    type Msg = SequencerSupervisorMsg;
    type State = SequencerSupervisorState;
    type Arguments = SequencerSupervisorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SequencerSupervisorState {
            current_actor: None,
            config: args.config,
            store: args.store,
            handle: args.handle,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SequencerSupervisorMsg::AdoptChild(actor) => {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    return Ok(());
                }
                state.current_actor = Some(actor.clone());
                state.publish_actor(Some(actor));
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let Some(current_actor) = state.current_actor.as_ref() else {
            return Ok(());
        };

        match message {
            SupervisionEvent::ActorStarted(actor) if actor.get_id() == current_actor.get_id() => {
                tracing::info!("sequencer actor started under supervisor");
            }
            SupervisionEvent::ActorFailed(actor, error)
                if actor.get_id() == current_actor.get_id() =>
            {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    tracing::warn!(
                        error = %error,
                        "sequencer actor failed during shutdown; not restarting"
                    );
                    state.current_actor = None;
                    state.publish_actor(None);
                    return Ok(());
                }
                tracing::error!(error = %error, "sequencer actor failed; attempting restart");
                state.restart_from_store(myself).await;
            }
            SupervisionEvent::ActorTerminated(actor, _, reason)
                if actor.get_id() == current_actor.get_id() =>
            {
                if state.handle.shutdown_requested.load(Ordering::Acquire) {
                    state.current_actor = None;
                    state.publish_actor(None);
                    tracing::info!("sequencer actor terminated during shutdown");
                    return Ok(());
                }
                if let Some(reason) = reason.as_deref() {
                    tracing::warn!(reason, "sequencer actor terminated; attempting restart");
                } else {
                    tracing::warn!("sequencer actor terminated; attempting restart");
                }
                state.restart_from_store(myself).await;
            }
            _ => {}
        }
        Ok(())
    }
}
