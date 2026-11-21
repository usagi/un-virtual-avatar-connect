"""既知のmanaged solo profileを公開channelなしの明示compositionへ一括移行する。

本適用では同じprofiles directoryへ日時付きbackupを作る。active treeは直接編集せず、sibling stagingで
生成・安全検査後にflowgraph directoryをswapする。dry-runと不一致profileはactive treeを変更しない。
"""

from __future__ import annotations

import argparse
import atexit
import datetime as dt
import json
import pathlib
import shutil
import tomllib


PUBLIC_CHANNELS = {"flowgraph.channel.emit", "flowgraph.ingress.channel_subscribe"}


def is_removed_feature(feature: object) -> bool:
    return feature in PUBLIC_CHANNELS or (isinstance(feature, str) and feature.startswith("flowgraph.library."))


def value(v):
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, list):
        return "[" + ", ".join(value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {value(x)}" for k, x in v.items()) + " }"
    raise TypeError(type(v))


def dump_doc(doc):
    out = []
    for section in ("meta", "graph"):
        if section in doc:
            out.append(f"[{section}]")
            out.extend(f"{k} = {value(v)}" for k, v in doc[section].items())
            out.append("")
    for item in doc.get("graph_inputs", []):
        out.append("[[graph.inputs]]")
        out.extend(f"{k} = {value(v)}" for k, v in item.items())
        out.append("")
    for item in doc.get("graph_outputs", []):
        out.append("[[graph.outputs]]")
        out.extend(f"{k} = {value(v)}" for k, v in item.items())
        out.append("")
    for node in doc.get("nodes", []):
        out.append("[[nodes]]")
        for k, v in node.items():
            if k != "properties":
                out.append(f"{k} = {value(v)}")
        if node.get("properties"):
            out.append("[nodes.properties]")
            out.extend(f"{k} = {value(v)}" for k, v in node["properties"].items())
        out.append("")
    for instance in doc.get("instances", []):
        out.append("[[instances]]")
        out.extend(f"{k} = {value(v)}" for k, v in instance.items())
        out.append("")
    for edge in doc.get("edges", []):
        out.append("[[edges]]")
        out.append(f"from = {value(edge['from'])}")
        out.append(f"to = {value(edge['to'])}")
        out.append("")
    return "\n".join(out).rstrip() + "\n"


def load(path):
    return tomllib.loads(path.read_text(encoding="utf-8-sig"))


def by_id(doc):
    return {n["id"]: n for n in doc.get("nodes", [])}


def node(node_id, feature, x, y, properties=None):
    result = {"id": node_id, "feature": feature, "position": [float(x), float(y)]}
    if properties:
        result["properties"] = properties
    return result


def edge(source, target):
    return {"from": source, "to": target}


def literal(node_id, feature, x, y, v):
    return node(node_id, feature, x, y, {"value": v})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("profile", type=pathlib.Path)
    parser.add_argument("--dry-run", action="store_true", help="stagingと安全検査だけを実行し、active profileは変更しない")
    args = parser.parse_args()
    profile = args.profile.resolve()
    if profile.parent.name != "profiles" or not (profile / "conf.toml").is_file():
        raise SystemExit("refusing non-managed-profile path")
    source_root = profile / "flowgraph.local"
    if not source_root.is_dir():
        raise SystemExit("flowgraph.local missing")

    conf = load(profile / "conf.toml")
    persona_id = conf["ai"]["personas"][0]["id"]
    solo = source_root / "solo"
    voice_old = load(solo / "voice.flowgraph.toml")
    chat_old = load(solo / "chat-tts.flowgraph.toml")
    ai_old = load(solo / "ai-tts.flowgraph.toml")
    translate_old = load(solo / "translate.flowgraph.toml")
    twitch_old = load(solo / "twitch-chat.flowgraph.toml")
    ocr_old = load(solo / "ocr.flowgraph.toml")
    reaction_old = load(source_root / "avatar-reaction.flowgraph.toml")
    points_old = load(source_root / "twitch-channel-points" / "main.flowgraph.toml")

    for doc in (voice_old, chat_old, ai_old, translate_old, twitch_old, ocr_old, reaction_old, points_old):
        if not any(n.get("feature") in PUBLIC_CHANNELS for n in doc.get("nodes", [])):
            raise SystemExit("legacy layout no longer matches; active profile was not changed")

    # Active treeへ一切書かず、sibling stagingで全生成と安全検査を完了してからdirectory swapする。
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    backup = None
    if not args.dry_run:
        backup = profile.with_name(profile.name + ".backup-explicit-" + stamp)
        shutil.copytree(profile, backup)
    staging_root = profile.parent / f".{profile.name}.flowgraph-staging-{stamp}"
    shutil.copytree(source_root, staging_root)
    cleanup_roots = [staging_root]

    def cleanup_staging():
        for path in cleanup_roots:
            if path.exists():
                shutil.rmtree(path, ignore_errors=True)

    atexit.register(cleanup_staging)
    root = staging_root
    solo = root / "solo"

    vn, cn, an, trn, twn, on = map(by_id, (voice_old, chat_old, ai_old, translate_old, twitch_old, ocr_old))

    translation = {
        "meta": {"title": "Japanese → English", "description": "型付きgraph interfaceを持つLibreTranslate module。", "tags": ["managed", "module", "translate"], "mode_groups": ["translation"], "default_enabled": True},
        "graph": {"kind": "module"},
        "graph_inputs": [{"name": "exec_in", "type": "exec", "to": "translate:exec_in"}, {"name": "text", "type": "string", "to": "translate:text"}],
        "graph_outputs": [{"name": "exec_out", "type": "exec", "from": "translate:on_success"}, {"name": "text", "type": "string", "from": "translate:translated"}],
        "nodes": [
            literal("from_lang", "flowgraph.literal.string", 80, 360, trn["from_ja"]["properties"]["value"]),
            literal("to_lang", "flowgraph.literal.string", 80, 420, trn["to_en"]["properties"]["value"]),
            literal("base_url", "flowgraph.literal.string", 80, 480, trn["libre_base"]["properties"]["value"]),
            node("translate", "flowgraph.translate.libre", 360, 220),
        ],
        "edges": [edge("from_lang:value", "translate:translate_from"), edge("to_lang:value", "translate:translate_to"), edge("base_url:value", "translate:base_url")],
    }

    voice_props = dict(vn["in"].get("properties", {}))
    voice = {
        "meta": {"title": "Voice → AI / BOS / translation", "description": "音声認識をAI観測、字幕、型付き翻訳moduleへ明示配線する。", "tags": ["managed", "voice", "ai", "composition"]},
        "nodes": [
            node("voice", "flowgraph.ingress.voice", 80, 220, voice_props),
            node("observe", "flowgraph.ai.observe", 400, 120, {"persona_id": persona_id}),
            literal("user_destination", "flowgraph.literal.string", 400, 360, "user"),
            node("user_bos", "flowgraph.bos.output", 700, 300),
            literal("user_en_destination", "flowgraph.literal.string", 980, 620, "user-en"),
            node("user_en_bos", "flowgraph.bos.output", 1260, 520),
            node("log_in", "flowgraph.util.log", 700, 120),
        ],
        "instances": [{"id": "translate_user", "graph": "translation-en/main", "position": [700.0, 500.0]}],
        "edges": [
            edge("voice:exec_out", "observe:exec_in"), edge("voice:content", "observe:content"),
            edge("voice:exec_out", "user_bos:exec_in"), edge("voice:content", "user_bos:content"), edge("user_destination:value", "user_bos:destination"),
            edge("voice:exec_out", "translate_user:exec_in"), edge("voice:content", "translate_user:text"),
            edge("translate_user:exec_out", "user_en_bos:exec_in"), edge("translate_user:text", "user_en_bos:content"), edge("user_en_destination:value", "user_en_bos:destination"),
            edge("voice:exec_out", "log_in:exec_in"), edge("voice:content", "log_in:value"),
        ],
    }

    assistant_nodes = [
        node("ai_response", "flowgraph.ingress.ai_response", 80, 180, {"persona_id": persona_id}),
        literal("ai_destination", "flowgraph.literal.string", 360, 80, an["ch_synth"]["properties"]["value"]),
        node("ai_bos", "flowgraph.bos.output", 680, 160),
        literal("ai_en_destination", "flowgraph.literal.string", 980, 480, "ai-en"),
        node("ai_en_bos", "flowgraph.bos.output", 1260, 400),
        node("ai_speech", "flowgraph.ingress.ai_speech", 80, 760, {"persona_id": persona_id}),
    ]
    for old_id, new_id, feature, x, y in [
        ("engine", "tts_engine", "flowgraph.literal.string", 360, 700), ("voice", "tts_voice", "flowgraph.literal.string", 360, 760),
        ("endpoint", "tts_endpoint", "flowgraph.literal.string", 360, 820), ("speed", "tts_speed", "flowgraph.literal.float", 360, 880),
        ("pitch", "tts_pitch", "flowgraph.literal.float", 360, 940), ("volume", "tts_volume", "flowgraph.literal.float", 360, 1000),
        ("lane", "tts_lane", "flowgraph.literal.string", 360, 1060), ("non_blocking", "tts_non_blocking", "flowgraph.literal.bool", 360, 1120),
        ("lip_sync_base_key", "lip_sync_base_key", "flowgraph.literal.string", 360, 1180), ("lip_sync_target", "lip_sync_target", "flowgraph.literal.string", 360, 1240),
    ]:
        assistant_nodes.append(literal(new_id, feature, x, y, an[old_id]["properties"]["value"]))
    assistant_nodes += [node("speak", "flowgraph.tts.speak", 700, 800), node("log_ok", "flowgraph.util.log", 1040, 760), node("log_err", "flowgraph.util.log", 1040, 900)]
    assistant = {
        "meta": {"title": "AI response → BOS / translation / TTS", "description": "AI確定全文とspeech segmentを用途別ingressから明示出力する。", "tags": ["managed", "ai", "tts", "composition"]},
        "nodes": assistant_nodes,
        "instances": [{"id": "translate_ai", "graph": "translation-en/main", "position": [680.0, 420.0]}],
        "edges": [
            edge("ai_response:exec_out", "ai_bos:exec_in"), edge("ai_response:content", "ai_bos:content"), edge("ai_response:meta", "ai_bos:meta"), edge("ai_destination:value", "ai_bos:destination"),
            edge("ai_response:exec_out", "translate_ai:exec_in"), edge("ai_response:content", "translate_ai:text"), edge("translate_ai:exec_out", "ai_en_bos:exec_in"), edge("translate_ai:text", "ai_en_bos:content"), edge("ai_en_destination:value", "ai_en_bos:destination"),
            edge("ai_speech:exec_out", "speak:exec_in"), edge("ai_speech:content", "speak:text"), edge("ai_speech:utterance_id", "speak:utterance_id"),
            edge("tts_engine:value", "speak:engine"), edge("tts_voice:value", "speak:voice"), edge("tts_endpoint:value", "speak:endpoint"), edge("tts_speed:value", "speak:speed"), edge("tts_pitch:value", "speak:pitch"), edge("tts_volume:value", "speak:volume"), edge("tts_lane:value", "speak:lane"), edge("tts_non_blocking:value", "speak:non_blocking"), edge("lip_sync_base_key:value", "speak:lip_sync_base_key"), edge("lip_sync_target:value", "speak:lip_sync_target_link_id"),
            edge("speak:on_success", "log_ok:exec_in"), edge("speak:audio_path", "log_ok:value"), edge("speak:on_error", "log_err:exec_in"), edge("speak:error", "log_err:value"),
        ],
    }

    twitch_nodes = [node("twitch_in", "flowgraph.ingress.twitch", 80, 200, dict(twn["in"].get("properties", {}))), literal("chat_destination", "flowgraph.literal.string", 340, 80, cn["ch_synth"]["properties"]["value"]), node("chat_bos", "flowgraph.bos.output", 620, 100)]
    for old_id, feature, y in [("engine", "flowgraph.literal.string", 400), ("voice", "flowgraph.literal.string", 460), ("endpoint", "flowgraph.literal.string", 520), ("speed", "flowgraph.literal.float", 580), ("pitch", "flowgraph.literal.float", 640), ("volume", "flowgraph.literal.float", 700), ("lane", "flowgraph.literal.string", 760), ("non_blocking", "flowgraph.literal.bool", 820)]:
        twitch_nodes.append(literal("tts_" + old_id, feature, 80, y, cn[old_id]["properties"]["value"]))
    twitch_nodes += [node("speak", "flowgraph.tts.speak", 620, 360), node("log_ok", "flowgraph.util.log", 940, 320), node("log_err", "flowgraph.util.log", 940, 460)]
    twitch = {
        "meta": {"title": "Twitch chat → BOS / TTS", "description": "Twitch chatを字幕と専用TTSへ明示配線する。", "tags": ["managed", "twitch", "tts"]},
        "nodes": twitch_nodes,
        "edges": [edge("twitch_in:exec_out", "chat_bos:exec_in"), edge("twitch_in:content", "chat_bos:content"), edge("twitch_in:source_actor", "chat_bos:source_actor"), edge("twitch_in:meta", "chat_bos:meta"), edge("chat_destination:value", "chat_bos:destination"), edge("twitch_in:exec_out", "speak:exec_in"), edge("twitch_in:content", "speak:text"), edge("twitch_in:trace_id", "speak:utterance_id"), edge("tts_engine:value", "speak:engine"), edge("tts_voice:value", "speak:voice"), edge("tts_endpoint:value", "speak:endpoint"), edge("tts_speed:value", "speak:speed"), edge("tts_pitch:value", "speak:pitch"), edge("tts_volume:value", "speak:volume"), edge("tts_lane:value", "speak:lane"), edge("tts_non_blocking:value", "speak:non_blocking"), edge("speak:on_success", "log_ok:exec_in"), edge("speak:audio_path", "log_ok:value"), edge("speak:on_error", "log_err:exec_in"), edge("speak:error", "log_err:value")],
    }

    ocr = {
        "meta": {"title": "Screenshot → OCR → AI / BOS", "description": "明示web triggerからOCRし、AI観測とBOSへ分岐する。", "tags": ["managed", "ocr", "ai"]},
        "nodes": [node("trigger", "flowgraph.ingress.web_input", 80, 200, dict(on["trigger"].get("properties", {}))), literal("title_regex", "flowgraph.literal.string", 80, 380, on["title_regex"]["properties"]["value"]), node("capture", "flowgraph.screenshot.capture", 360, 200), literal("client_only", "flowgraph.literal.bool", 360, 120, on["client_only"]["properties"]["value"]), literal("lang", "flowgraph.literal.string", 360, 460, on["lang"]["properties"]["value"]), node("ocr", "flowgraph.ocr.recognize", 680, 240), node("observe", "flowgraph.ai.observe", 1000, 180, {"persona_id": persona_id}), literal("destination", "flowgraph.literal.string", 760, 520, on["ch_out"]["properties"]["value"]), node("bos", "flowgraph.bos.output", 1000, 360), node("log_capture_err", "flowgraph.util.log", 680, 400), node("log_ocr_err", "flowgraph.util.log", 1000, 480)],
        "edges": [edge("trigger:exec_out", "capture:exec_in"), edge("title_regex:value", "capture:target_title_regex"), edge("client_only:value", "capture:client_only"), edge("capture:on_success", "ocr:exec_in"), edge("capture:data_url", "ocr:source"), edge("lang:value", "ocr:lang"), edge("capture:on_error", "log_capture_err:exec_in"), edge("capture:error", "log_capture_err:value"), edge("ocr:on_success", "observe:exec_in"), edge("ocr:text", "observe:content"), edge("ocr:on_success", "bos:exec_in"), edge("ocr:text", "bos:content"), edge("destination:value", "bos:destination"), edge("ocr:on_error", "log_ocr_err:exec_in"), edge("ocr:error", "log_ocr_err:value")],
    }

    reaction_nodes = by_id(reaction_old)
    replacement = {"emit_ai_request": "flowgraph.ai.observe", "emit_effect": "flowgraph.bos.output", "emit_observation": "flowgraph.bos.output"}
    keep = []
    for n in points_old["nodes"]:
        if n["id"] in {"user_channel", "avatar_channel", "avatar_intent_string", "emit_avatar_reaction"}:
            continue
        n = dict(n)
        if n["id"] in replacement:
            n["feature"] = replacement[n["id"]]
        keep.append(n)
    keep.append(node("play_avatar_reaction", "flowgraph.avatar.intent_play_unmfz", 1360, 300))
    for old_id in ("unmfz_base_key", "unmfz_topic_mode", "unmfz_schema_major", "dry_run", "playback_fps"):
        copied = dict(reaction_nodes[old_id]); copied["id"] = "reaction_" + old_id; keep.append(copied)
    drop_sources = {"user_channel:value", "avatar_intent_string:text", "avatar_channel:value"}
    drop_targets = {"emit_ai_request:channel", "emit_avatar_reaction:exec_in", "emit_avatar_reaction:content", "emit_avatar_reaction:channel", "emit_avatar_reaction:source_actor", "emit_avatar_reaction:meta", "avatar_intent_string:value", "emit_effect:channel", "emit_observation:channel"}
    points_edges = [dict(e) for e in points_old["edges"] if e["from"] not in drop_sources and e["to"] not in drop_targets]
    points_edges += [edge("effect_channel:value", "emit_effect:destination"), edge("observe_channel:value", "emit_observation:destination"), edge("route:then", "play_avatar_reaction:exec_in"), edge("avatar_intent:value", "play_avatar_reaction:intent"), edge("reaction_unmfz_base_key:value", "play_avatar_reaction:base_key_expr"), edge("reaction_unmfz_topic_mode:value", "play_avatar_reaction:topic_mode"), edge("reaction_unmfz_schema_major:value", "play_avatar_reaction:schema_major"), edge("reaction_dry_run:value", "play_avatar_reaction:dry_run"), edge("reaction_playback_fps:value", "play_avatar_reaction:fps")]
    points = {"meta": dict(points_old.get("meta", {})), "nodes": keep, "edges": points_edges}

    # Dedicated AI output ingress replaces the old public channel subscription.
    for rel, feature in [("avatar-pose.flowgraph.toml", "flowgraph.ingress.ai_avatar_pose"), ("avatar-reaction.flowgraph.toml", "flowgraph.ingress.ai_avatar_intent")]:
        path = root / rel
        text = path.read_text(encoding="utf-8-sig")
        text = text.replace('feature = "flowgraph.ingress.channel_subscribe"', f'feature = "{feature}"', 1)
        text = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("properties.channels") and not line.lstrip().startswith("properties.require_final") and not line.lstrip().startswith("properties.ignore_flowgraph_echo")) + "\n"
        path.write_text(text, encoding="utf-8")

    outputs = {root / "translation-en" / "main.flowgraph.toml": translation, root / "voice" / "main.flowgraph.toml": voice, root / "assistant-output" / "main.flowgraph.toml": assistant, root / "twitch-chat" / "main.flowgraph.toml": twitch, root / "ocr" / "main.flowgraph.toml": ocr, root / "twitch-channel-points" / "main.flowgraph.toml": points}
    for path, doc in outputs.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(dump_doc(doc), encoding="utf-8")

    for name in ("voice.flowgraph.toml", "chat-tts.flowgraph.toml", "ai-tts.flowgraph.toml", "translate.flowgraph.toml", "twitch-chat.flowgraph.toml", "ocr.flowgraph.toml"):
        (solo / name).unlink()
    old_test = root / "twitch-channel-points" / "main.flowgraph.test.toml"
    if old_test.exists():
        old_test.unlink()

    remaining = []
    for path in root.rglob("*.flowgraph.toml"):
        doc = load(path)
        for n in doc.get("nodes", []):
            if is_removed_feature(n.get("feature")):
                remaining.append((path, n.get("id"), n.get("feature")))
        if "library_uses" in doc.get("meta", {}):
            remaining.append((path, "meta", "library_uses"))
    if remaining:
        raise SystemExit(f"migration staging left {len(remaining)} removed declarations; active profile was not changed; backup={backup}")
    active_flowgraph_files = len(list(root.rglob("*.flowgraph.toml")))
    if args.dry_run:
        cleanup_staging()
        cleanup_roots.clear()
        print(json.dumps({"ok": True, "dry_run": True, "backup": None, "active_flowgraph_files": active_flowgraph_files, "removed_declarations": 0}, ensure_ascii=False))
        return

    swap_root = profile.parent / f".{profile.name}.flowgraph-swap-{stamp}"
    source_root.rename(swap_root)
    try:
        staging_root.rename(source_root)
    except BaseException:
        swap_root.rename(source_root)
        raise
    cleanup_roots.clear()
    shutil.rmtree(swap_root)
    print(json.dumps({"ok": True, "dry_run": False, "backup": str(backup), "active_flowgraph_files": active_flowgraph_files, "removed_declarations": 0}, ensure_ascii=False))


if __name__ == "__main__":
    main()
