use crate::engine::session_llm::read_session_llm;
use crate::interrupt::finalize::{
    finalize_stream_cancel, FinalizeStreamCancelParams, MainSessionSink,
};
use crate::interrupt::AbortController;
use crate::message::{assistant_from_completion, parse_tool_call_input};
use crate::turn::StreamingToolDispatch;
use crate::turn::TurnContext;
use crate::{AgentEngine, AgentError, ContentBlockKind, Event, TerminalReason};
use novel_deepseek::{
    is_output_truncated, ChatClient, LlmChatMessage, LlmCompletion, LlmToolCall, StreamEvent,
    StreamOutcome,
};
use novel_logging::LogEvent;
use novel_tools::AbortSignal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// DeepSeek thinking mode often ends a stream with only `reasoning_content` and no
/// `tool_calls`, while the model still intends to act on the next inner iteration.
pub(crate) fn should_continue_inner_after_completion(completion: &LlmCompletion) -> bool {
    if !completion.tool_calls.is_empty() {
        return false;
    }
    if is_output_truncated(completion.stop_reason.as_deref()) {
        return false;
    }
    let content_empty = completion
        .content
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let has_reasoning = completion
        .reasoning_content
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    content_empty && has_reasoning
}

pub(crate) async fn run_abort_bridge(ac: Arc<AbortController>, tx: watch::Sender<AbortSignal>) {
    let mut rx = ac.subscribe();
    loop {
        if rx.changed().await.is_err() {
            break;
        }
        let _ = tx.send(crate::interrupt::map_abort_signal(*rx.borrow()));
    }
}

pub(crate) enum LlmCallOutcome {
    Continue(LlmCompletion),
    Aborted(TerminalReason),
}

impl AgentEngine {
    // ── LLM call + streaming tool execution (unified path) ────

    pub(crate) async fn call_llm_and_execute(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, serde_json::Value)],
        turn_ctx: &TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<LlmCallOutcome, AgentError> {
        if self.llm.is_some() {
            self.run_live_llm_stream_turn(
                messages,
                tools,
                turn_ctx,
                event_tx,
                persist_tool_messages,
            )
            .await
        } else {
            self.run_offline_llm_turn(messages, turn_ctx, event_tx)
                .await
        }
    }

    async fn finish_live_stream_tool_batch(
        &mut self,
        dispatch_arc: Arc<Mutex<StreamingToolDispatch>>,
        ctx: &novel_tools::ToolContext,
        completion: &LlmCompletion,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<LlmCallOutcome, AgentError> {
        let (executed_specs, skip_result_events, denied_specs, mut executor) = {
            let mut dispatch = dispatch_arc.lock().map_err(|_| {
                AgentError::Validation("streaming tool dispatch lock poisoned".into())
            })?;
            for tc in &completion.tool_calls {
                if !dispatch.handled_ids.contains(&tc.id) {
                    dispatch.handle_ready(&self.shared.registry, ctx, event_tx, tc.clone(), true);
                } else if let Some(tx) = event_tx {
                    let input = parse_tool_call_input(&tc.arguments, &tc.id, &tc.name);
                    let needs_approval = self.pending_tools.contains_key(&tc.id)
                        || dispatch.pending_specs.contains_key(&tc.id);
                    // Already handled during stream: only re-notify UI for pending approval.
                    // A bare ToolCallRequest maps to input_complete and would clobber done status.
                    if needs_approval {
                        let _ = tx.send(Event::ToolCallRequest {
                            tool_call_id: tc.id.clone(),
                            name: tc.name.clone(),
                            input,
                            needs_approval,
                        });
                    }
                }
            }
            for (_, spec) in dispatch.pending_specs.drain() {
                self.pending_tools.insert(spec.id.clone(), spec);
            }
            self.set_interruptible_tool_in_progress(
                dispatch
                    .executor_mut()
                    .is_some_and(|e| e.has_interruptible_tool_in_progress()),
                event_tx,
            );
            dispatch.poll_ui_results(
                &self.shared.registry,
                event_tx,
                Some((&self.shared.session.id, &self.shared.session.db)),
            );
            let executor = dispatch.take_executor().ok_or_else(|| {
                AgentError::Validation("streaming tool executor already taken".into())
            })?;
            let executed_specs = std::mem::take(&mut dispatch.executed_specs);
            let skip_result_events = std::mem::take(&mut dispatch.ui_result_emitted);
            let denied_specs = std::mem::take(&mut dispatch.denied_specs);
            (executed_specs, skip_result_events, denied_specs, executor)
        };

        let mut results = executor.get_remaining_results().await;
        for (id, (_, reason)) in denied_specs {
            results.push((id, Err(novel_tools::ToolError::PermissionDenied(reason))));
        }
        self.set_interruptible_tool_in_progress(false, event_tx);
        for spec in &executed_specs {
            self.pending_tools.remove(&spec.id);
        }

        let assistant = assistant_from_completion(completion);
        self.persist_message_alloc(&assistant)?;
        self.messages.push(assistant);

        let tool_call_order: Vec<String> = completion
            .tool_calls
            .iter()
            .map(|tc| tc.id.clone())
            .collect();

        if self.interrupt_requested() {
            let _ = self
                .execute_stream_results(
                    results,
                    &executed_specs,
                    &tool_call_order,
                    event_tx,
                    persist_tool_messages,
                    &skip_result_events,
                )
                .await?;
            return Ok(LlmCallOutcome::Aborted(TerminalReason::AbortedTools));
        }

        let _ = self
            .execute_stream_results(
                results,
                &executed_specs,
                &tool_call_order,
                event_tx,
                persist_tool_messages,
                &skip_result_events,
            )
            .await?;

        Ok(LlmCallOutcome::Continue(completion.clone()))
    }

    async fn run_offline_llm_turn(
        &mut self,
        messages: &[LlmChatMessage],
        turn_ctx: &TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
    ) -> Result<LlmCallOutcome, AgentError> {
        let completion = ChatClient::offline_complete(messages);
        if let Some(tx) = event_tx {
            if let Some(content) = &completion.content {
                let _ = tx.send(Event::ContentBlockDelta {
                    message_id: String::new(),
                    index: 0,
                    delta: content.clone(),
                    kind: ContentBlockKind::Text,
                });
            }
            let _ = tx.send(Event::AssistantSegmentComplete {
                segment_index: turn_ctx.inner_turn,
                fork_run_id: None,
            });
        }
        Ok(LlmCallOutcome::Continue(completion))
    }

    async fn run_live_llm_stream_turn(
        &mut self,
        messages: &[LlmChatMessage],
        tools: &[(String, String, serde_json::Value)],
        turn_ctx: &TurnContext,
        event_tx: Option<&mpsc::UnboundedSender<Event>>,
        persist_tool_messages: bool,
    ) -> Result<LlmCallOutcome, AgentError> {
        let max_concurrent = self.shared.settings.agent.max_tool_concurrency;
        let session_id = self.shared.session.id.clone();
        let model = self.shared.settings.model.model.clone();
        tracing::debug!(
            session_id = %session_id,
            inner_turn = turn_ctx.inner_turn,
            message_count = messages.len(),
            tool_schema_count = tools.len(),
            "llm_request_start"
        );
        self.audit_log(LogEvent::LlmRequest {
            session_id: session_id.clone(),
            model: model.clone(),
            streaming: true,
        });
        let tx = event_tx.cloned();
        let audit = self.shared.audit.clone();
        let cancel_flag = self.shared.abort_controller.cancel_flag();
        let initial_abort = crate::interrupt::map_abort_signal(self.abort_reason());
        let (abort_tool_tx, abort_tool_rx) = novel_tools::abort_channel();
        let _ = abort_tool_tx.send(initial_abort);
        let bridge = tokio::spawn(run_abort_bridge(
            Arc::clone(&self.shared.abort_controller),
            abort_tool_tx,
        ));

        let ctx = self.tool_context();
        let dispatch_arc = Arc::new(Mutex::new(StreamingToolDispatch::new(
            Arc::clone(&self.shared.registry),
            ctx.clone(),
            max_concurrent,
            abort_tool_rx,
        )));

        let stream_done = Arc::new(AtomicBool::new(false));
        let stream_done_poll = Arc::clone(&stream_done);
        let dispatch_poll = Arc::clone(&dispatch_arc);
        let registry_poll = Arc::clone(&self.shared.registry);
        let event_tx_poll = event_tx.cloned();
        let session_id_poll = self.shared.session.id.clone();
        let db_poll = self.shared.session.db.clone();
        let poll_handle = tokio::spawn(async move {
            while !stream_done_poll.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(40)).await;
                if let Ok(mut d) = dispatch_poll.lock() {
                    d.poll_ui_results(
                        &registry_poll,
                        event_tx_poll.as_ref(),
                        Some((&session_id_poll, &db_poll)),
                    );
                }
            }
        });

        let dispatch_cb = Arc::clone(&dispatch_arc);
        let registry_cb = Arc::clone(&self.shared.registry);
        let ctx_cb = ctx.clone();
        let event_tx_cb = event_tx.cloned();
        let on_tool = move |tc: LlmToolCall| {
            if let Ok(mut d) = dispatch_cb.lock() {
                d.handle_ready(&registry_cb, &ctx_cb, event_tx_cb.as_ref(), tc, false);
            }
        };

        let Some(client) = self.llm.as_mut() else {
            return Err(AgentError::Validation(
                "LLM client not initialized for live stream".into(),
            ));
        };
        let stream_result = client
            .create_stream(
                messages,
                tools,
                self.shared.settings.model.max_output_tokens,
                move |ev: StreamEvent| {
                    if let Some(ref tx) = tx {
                        forward_main_stream_event(tx, audit.as_deref(), ev);
                    }
                },
                Some(on_tool),
                Some(cancel_flag),
            )
            .await;

        stream_done.store(true, Ordering::SeqCst);
        let _ = poll_handle.await;
        bridge.abort();

        let (completion, stream_aborted, bg_usage) = match stream_result {
            Err(e) => {
                tracing::warn!(error = %e, "llm_request_failed");
                self.audit_error(e.to_string(), true);
                return Err(AgentError::Llm(e));
            }
            Ok(StreamOutcome::Complete(c)) => (c, false, None),
            Ok(StreamOutcome::Cancelled {
                partial,
                background_usage,
            }) => (partial, true, Some(background_usage)),
        };

        if stream_aborted || self.interrupt_requested() {
            if let Ok(mut dispatch) = dispatch_arc.lock() {
                dispatch.discard();
            }
            let shared = self.shared.clone();
            let llm_snap = read_session_llm(&shared);
            let llm_messages = messages.to_vec();
            let tool_schemas = tools.to_vec();
            let mut sink = MainSessionSink { engine: self };
            let usage = finalize_stream_cancel(FinalizeStreamCancelParams {
                sink: &mut sink,
                partial: completion,
                llm_messages,
                tool_schemas,
                background_usage: bg_usage,
                llm_snap,
                shared,
                event_tx,
                update_context_snapshot: true,
            })
            .await?;
            self.last_turn_usage = usage.clone();
            if let Some(u) = usage.as_ref() {
                tracing::debug!(
                    cache_hit = u.cache_hit_tokens,
                    cache_miss = u.cache_miss_tokens,
                    completion = u.completion_tokens,
                    "token_usage_recorded_stream_abort"
                );
            }
            return Ok(LlmCallOutcome::Aborted(TerminalReason::AbortedStreaming));
        }

        tracing::debug!(
            session_id = %session_id,
            tool_call_count = completion.tool_calls.len(),
            has_usage = completion.usage.is_some(),
            stream_aborted,
            "llm_request_complete"
        );

        self.record_usage(&completion, event_tx);

        if let Some(tx) = event_tx {
            let _ = tx.send(Event::AssistantSegmentComplete {
                segment_index: turn_ctx.inner_turn,
                fork_run_id: None,
            });
        }

        if completion.tool_calls.is_empty() {
            if let Ok(mut dispatch) = dispatch_arc.lock() {
                dispatch.drain_pending_specs(self);
            }
            return Ok(LlmCallOutcome::Continue(completion));
        }

        return self
            .finish_live_stream_tool_batch(
                dispatch_arc,
                &ctx,
                &completion,
                event_tx,
                persist_tool_messages,
            )
            .await;
    }
}

pub(crate) fn forward_main_stream_event(
    tx: &mpsc::UnboundedSender<Event>,
    audit: Option<&novel_logging::AuditLogger>,
    ev: StreamEvent,
) {
    match ev {
        StreamEvent::ContentBlockDelta { delta, kind, .. } => {
            let _ = tx.send(Event::ContentBlockDelta {
                message_id: String::new(),
                index: 0,
                delta,
                kind,
            });
        }
        StreamEvent::ToolUseStarted {
            tool_call_id, name, ..
        } => {
            let _ = tx.send(Event::ToolUseStarted { tool_call_id, name });
        }
        StreamEvent::ToolInputDelta {
            tool_call_id,
            delta,
        } => {
            let _ = tx.send(Event::ToolInputDelta {
                tool_call_id,
                delta,
            });
        }
        StreamEvent::MessageStop { .. } => {}
        StreamEvent::StreamError { message, .. } => {
            tracing::warn!(error = %message, "llm_stream_error");
            if let Some(a) = audit {
                let _ = a.log(&LogEvent::Error {
                    message: message.clone(),
                    recoverable: true,
                });
            }
            let _ = tx.send(Event::Error {
                message,
                recoverable: true,
            });
        }
    }
}
