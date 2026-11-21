//! STTのpartial字幕をStateへ非同期反映する直列queue。

enum Command {
	Update(String),
	Finish(String),
	Clear,
}

/// STT threadをStateの非同期lockから分離するpartial字幕更新handle。
pub(crate) struct VoicePartialPreview {
	tx: Option<tokio::sync::mpsc::UnboundedSender<Command>>,
}

impl VoicePartialPreview {
	pub(crate) fn spawn(
		rt: &tokio::runtime::Handle,
		state: std::sync::Weak<tokio::sync::RwLock<crate::state::State>>,
		channel: String,
	) -> Self {
		let channel = channel.trim().to_string();
		if channel.is_empty() {
			return Self { tx: None };
		}
		let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
		rt.spawn(async move {
			let mut datum_id = None;
			while let Some(command) = rx.recv().await {
				let Some(state) = state.upgrade() else {
					break;
				};
				match command {
					Command::Update(text) => {
						let updated = if let Some(id) = datum_id {
							state.read().await.update_channel_datum_content_by_id(id, text.clone()).await
						} else {
							false
						};
						if !updated {
							let datum = crate::ChannelDatum::new(channel.clone(), text)
								.with_source(crate::state::DataSource::new("voice.partial"))
								.with_meta("utterance_state", "partial");
							let id = datum.get_id();
							state.read().await.push_channel_datum_output_only(datum).await;
							datum_id = Some(id);
						}
					}
					Command::Finish(text) => {
						if let Some(id) = datum_id.take() {
							let _ = state.read().await.update_channel_datum_content_by_id(id, text).await;
						}
					}
					Command::Clear => {
						if let Some(id) = datum_id.take() {
							let _ = state.read().await.update_channel_datum_content_by_id(id, String::new()).await;
						}
					}
				}
			}
		});
		Self { tx: Some(tx) }
	}

	fn send(&self, command: Command) {
		if let Some(tx) = &self.tx {
			let _ = tx.send(command);
		}
	}

	pub(crate) fn update(&self, text: String) {
		if !text.is_empty() {
			self.send(Command::Update(text));
		}
	}

	pub(crate) fn finish(&self, text: String) {
		self.send(Command::Finish(text));
	}

	pub(crate) fn clear(&self) {
		self.send(Command::Clear);
	}

	#[cfg(test)]
	pub(crate) fn disabled() -> Self {
		Self { tx: None }
	}
}
