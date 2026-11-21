export interface DictionarySetupInput {
    expected_source: string;
    revision: string;
    key: string;
    label: string;
    existing_path?: string | null;
}

export interface DictionarySetupPreview {
    source_path: string;
    revision: string;
    dictionary_path: string;
    existing_revision: string | null;
    existing_rows: number | null;
    creates_file: boolean;
    saved: false;
    runtime_connected: false;
    review_revision: string;
    storage_scope: "profile_directory" | "external_file";
}

export interface DictionarySetupSaveInput {
    registration: DictionarySetupInput;
    review_revision: string;
    external_file_confirmed: boolean;
}

export interface DictionarySetupSaved {
    source_path: string;
    dictionary_path: string;
    revision: string;
    saved: true;
    restart_required: true;
    /** Registration does not connect a graph; not a survey of existing routes. */
    runtime_connected: false;
    storage_scope: "profile_directory" | "external_file";
}

const signature = (input: DictionarySetupInput) => JSON.stringify([
    input.expected_source, input.revision, input.key, input.label, input.existing_path ?? null,
]);

export async function prepareDictionarySetupReview(
    origin: string,
    input: DictionarySetupInput,
    load: (input: DictionarySetupInput) => Promise<DictionarySetupPreview>,
): Promise<DictionarySetupReview> {
    // Capture before awaiting: a late reply must retain the submitted form,
    // not the user's newer input or newly selected profile.
    const captured = { ...input };
    const view = await load({ ...captured });
    return new DictionarySetupReview(origin, captured, view);
}

/** Layout-independent review. Each request consumes its review before sending,
 * so an unknown network result cannot be silently submitted a second time. */
export class DictionarySetupReview {
    readonly origin: string;
    readonly input: Readonly<DictionarySetupInput>;
    readonly preview: Readonly<DictionarySetupPreview>;
    private used = false;

    constructor(origin: string, input: DictionarySetupInput, preview: DictionarySetupPreview) {
        this.origin = new URL(origin).origin;
        this.input = Object.freeze({ ...input });
        this.preview = Object.freeze({ ...preview });
        if (preview.source_path !== input.expected_source || preview.revision !== input.revision || !preview.review_revision) {
            throw new Error("確認結果の操作対象が一致しません。入力を保持して確認し直してください。");
        }
    }

    matches(origin: string, current: DictionarySetupInput): boolean {
        return !this.used && new URL(origin).origin === this.origin && signature(current) === signature(this.input);
    }

    createSaveRequest(origin: string, current: DictionarySetupInput, externalConfirmed: boolean): DictionarySetupSaveInput {
        if (!this.matches(origin, current)) {
            throw new Error("確認後に入力または操作対象が変わりました。内容を確認し直してください。");
        }
        if (this.preview.storage_scope === "external_file" && !externalConfirmed) {
            throw new Error("外部辞書と同じファイルを使うことを確認してください。");
        }
        this.used = true;
        return { registration: { ...this.input }, review_revision: this.preview.review_revision, external_file_confirmed: externalConfirmed };
    }

    /** Keep the receipt attached to the submitted target. A success response
     * must never replace current form data or a newer configuration revision.
     * Refresh means fetch current metadata, not blindly install saved.revision. */
    resolveSaved(origin: string, current: DictionarySetupInput, saved: DictionarySetupSaved) {
        if (!this.used || saved.source_path !== this.input.expected_source
            || saved.dictionary_path !== this.preview.dictionary_path
            || saved.storage_scope !== this.preview.storage_scope
            || saved.saved !== true || saved.restart_required !== true
            || saved.runtime_connected !== false || !saved.revision) {
            throw new Error("登録結果を操作対象と照合できません。入力を保持して保存先を確認してください。");
        }
        const sameTarget = new URL(origin).origin === this.origin
            && current.expected_source === this.input.expected_source;
        return Object.freeze({
            origin: this.origin,
            receipt: Object.freeze({ ...saved }),
            refreshCurrentSource: sameTarget && current.revision === this.input.revision,
            submittedInputUnchanged: sameTarget && signature(current) === signature(this.input),
        });
    }
}
