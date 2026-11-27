//! A scripted Herdr stand-in: what the Agent reports each scan and how `agent.prompt` behaves.
#![allow(dead_code)]
use forge_app::{
    AgentAdapterError, AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, AgentPrompt, AgentPromptOutcome,
    AgentTurn, DispatchWorkspace,
};
use std::sync::{Arc, Mutex};

/// The scripted agent: what Herdr would report and how `agent.prompt` behaves.
pub struct Script {
    pub turn: Mutex<AgentTurn>,
    pub prompts: Mutex<Vec<AgentPrompt>>,
    pub prompt_result: Mutex<Result<AgentPromptOutcome, AgentAdapterError>>,
    pub retirements: Mutex<u32>,
    pub retire_result: Mutex<Result<(), AgentAdapterError>>,
}
impl Script {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            turn: Mutex::new(turn(AgentObservation::Working, Some(0))),
            prompts: Mutex::new(vec![]),
            prompt_result: Mutex::new(Ok(AgentPromptOutcome::Delivered)),
            retirements: Mutex::new(0),
            retire_result: Mutex::new(Ok(())),
        })
    }
    pub fn report(&self, observation: AgentObservation, seq: u64) {
        *self.turn.lock().unwrap() = turn(observation, Some(seq));
    }
    pub fn sent(&self) -> Vec<String> {
        self.prompts
            .lock()
            .unwrap()
            .iter()
            .map(|p| p.text.clone())
            .collect()
    }
}
pub fn turn(observation: AgentObservation, completion_seq: Option<u64>) -> AgentTurn {
    AgentTurn {
        observation,
        completion_seq,
    }
}
pub struct Factory(pub Arc<Script>);
impl AgentDispatchAdapterFactory for Factory {
    fn adapter_for(
        &self,
        _: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, AgentAdapterError> {
        Ok(Box::new(Adapter(self.0.clone())))
    }
}
struct Adapter(Arc<Script>);
impl AgentDispatchAdapter for Adapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn verify_workspace(&self, id: &str) -> Result<DispatchWorkspace, AgentAdapterError> {
        Ok(DispatchWorkspace { id: id.into() })
    }
    fn provision(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Ok(())
    }
    fn observe(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        Ok(self.0.turn.lock().unwrap().observation)
    }
    fn observe_turn(&self, _: &AgentDispatchContext) -> Result<AgentTurn, AgentAdapterError> {
        Ok(*self.0.turn.lock().unwrap())
    }
    fn prompt(
        &self,
        _: &AgentDispatchContext,
        prompt: &AgentPrompt,
    ) -> Result<AgentPromptOutcome, AgentAdapterError> {
        self.0.prompts.lock().unwrap().push(prompt.clone());
        self.0.prompt_result.lock().unwrap().clone()
    }
    fn recover(&self, _: &AgentDispatchContext) -> Result<AgentObservation, AgentAdapterError> {
        unreachable!()
    }
    fn retire(
        &self,
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        *self.0.retirements.lock().unwrap() += 1;
        self.0.retire_result.lock().unwrap().clone()
    }
}
