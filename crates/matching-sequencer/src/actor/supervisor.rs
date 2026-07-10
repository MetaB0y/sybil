use super::*;
use ractor::SupervisionEvent;

#[derive(Clone)]
pub(super) struct SequencerHandleInner {
    pub(super) actor: Arc<RwLock<Option<ActorRef<SequencerMsg>>>>,
    pub(super) block_broadcast: broadcast::Sender<SealedBlock>,
    pub(super) mailbox_monitor: MailboxMonitor,
    pub(super) shutdown_requested: Arc<AtomicBool>,
}

impl SequencerHandleInner {
    pub(super) fn publish_actor(&self, actor: Option<ActorRef<SequencerMsg>>) {
        self.mailbox_monitor.reset();
        *self
            .actor
            .write()
            .expect("sequencer actor ref lock poisoned") = actor;
    }
}

pub(super) struct SequencerSupervisor;

pub(super) struct SequencerSupervisorArgs {
    pub(super) config: SequencerConfig,
    pub(super) store: Option<Arc<crate::store::Store>>,
    pub(super) oracle: Arc<dyn Oracle>,
    pub(super) handle: SequencerHandleInner,
}

pub(super) struct SequencerSupervisorState {
    current_actor: Option<ActorRef<SequencerMsg>>,
    config: SequencerConfig,
    store: Option<Arc<crate::store::Store>>,
    oracle: Arc<dyn Oracle>,
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
            tracing::error!(
                "sequencer actor exited without a persistent store; restart unavailable"
            );
            return;
        };

        let restored = match store
            .load_state_with_fill_history_cap(self.config.max_fill_history_per_account)
            .await
        {
            Ok(state) => state,
            Err(error) => {
                tracing::error!(error = %error, "failed to load sequencer snapshot for restart");
                return;
            }
        };

        let Some(state) = restored else {
            tracing::error!("no persisted sequencer snapshot available for restart");
            return;
        };

        if self.handle.shutdown_requested.load(Ordering::Acquire) {
            return;
        }

        let sequencer = BlockSequencer::restore(state, self.oracle.clone(), self.config.clone());

        match self.spawn_child(myself, sequencer).await {
            Ok(()) => tracing::warn!("sequencer actor restarted from persistent snapshot"),
            Err(error) => {
                tracing::error!(error = %error, "failed to restart sequencer actor from snapshot");
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
            oracle: args.oracle,
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
