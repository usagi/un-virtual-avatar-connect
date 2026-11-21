/** Saved configuration projection; these fields do not report live capture. */
export type ObservationRule = "excluded" | "all" | "own_utterance" | "trigger" | "additional" | "unobserved";
export interface PerceptionObserver {
	persona_index: number;
	persona_id: string | null;
	display_name: string | null;
	enabled: boolean;
	rule: ObservationRule | null;
}
export interface PerceptionSource {
	id: string;
	scene_name: string;
	source_name: string;
	enabled: boolean;
	refresh_before_reply: boolean;
	active_modes: string[];
	channel: string;
	shared_source_ids: string[];
	observers: PerceptionObserver[];
	connection: {server:string|null;secure:boolean;custom_path_or_query:boolean;environment_password:boolean;saved_password:boolean;local_obs_password:boolean};
}
export interface PerceptionView {
	revision: string;
	sources: PerceptionSource[];
	archived_sources: PerceptionSource[];
	history: PerceptionHistory[];
}
export interface PerceptionHistory {
	persona_index: number;
	persona_id: string | null;
	display_name: string | null;
	turn_management_enabled: boolean;
	visual_history_enabled: boolean;
	past_screen_image_enabled: boolean;
}
export interface PerceptionSourcePatch {
	enabled?: boolean;
	refresh_before_reply?: boolean;
	scene_name?: string;
	source_name?: string;
	active_modes?: string[];
}
export interface PerceptionSourceCreate {
	id:string;
	scene_name:string;
	source_name:string;
	url:string;
	password_env:string|null;
	use_local_obs_config_password:boolean;
}
export type PerceptionCredentialReplacement =
	| {kind:'environment';name:string} | {kind:'local_obs'} | {kind:'password';value:string} | {kind:'none'};
export type PerceptionEdit =
	| { kind:'connection';source_id:string;url:string|null;credential:PerceptionCredentialReplacement|null }
	| { kind: "source"; source_id: string; patch: PerceptionSourcePatch }
	| { kind: "assignment"; source_id: string; persona_index: number; shared_source_ids: string[]; observe: boolean }
	| { kind: "history"; persona_index: number; visual_history_enabled?: boolean; past_screen_image_enabled?: boolean }
	| { kind: "create"; source: PerceptionSourceCreate }
	| { kind: "source_lifecycle"; source_id: string; restore: boolean; shared_source_ids: string[]; observer_indices: number[] };
export interface PerceptionSaveResult {
	revision: string;
	restart_required: boolean;
}
