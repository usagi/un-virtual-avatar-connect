from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import textwrap
import tomllib
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
MIGRATOR = REPO_ROOT / "scripts" / "migrate-managed-flowgraph-explicit.py"
REMOVED = {"flowgraph.channel.emit", "flowgraph.ingress.channel_subscribe"}


def write(path: pathlib.Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(textwrap.dedent(content).strip() + "\n", encoding="utf-8")


def node(node_id: str, feature: str, value: object | None = None) -> str:
    properties = ""
    if value is not None:
        properties = f"\n[nodes.properties]\nvalue = {json.dumps(value)}"
    return f'[[nodes]]\nid = "{node_id}"\nfeature = "{feature}"\nposition = [0.0, 0.0]{properties}\n'


def tree_hash(root: pathlib.Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()


def make_legacy_profile(profiles: pathlib.Path, name: str = "fixture") -> pathlib.Path:
    profile = profiles / name
    root = profile / "flowgraph.local"
    write(profile / "conf.toml", '[ai]\n[[ai.personas]]\nid = "test-persona"')

    def legacy(rel: str, nodes: list[str]) -> None:
        write(root / rel, "\n".join(nodes + [node("legacy", "flowgraph.channel.emit")]))

    legacy("solo/voice.flowgraph.toml", [node("in", "flowgraph.ingress.voice")])
    legacy(
        "solo/chat-tts.flowgraph.toml",
        [node("ch_synth", "flowgraph.literal.string", "chat-synth")]
        + [node(key, feature, value) for key, feature, value in (
            ("engine", "flowgraph.literal.string", "coeiroink"),
            ("voice", "flowgraph.literal.string", "fixture"),
            ("endpoint", "flowgraph.literal.string", "http://127.0.0.1:50032"),
            ("speed", "flowgraph.literal.float", 1.0),
            ("pitch", "flowgraph.literal.float", 0.0),
            ("volume", "flowgraph.literal.float", 1.0),
            ("lane", "flowgraph.literal.string", "chat"),
            ("non_blocking", "flowgraph.literal.bool", True),
        )],
    )
    legacy(
        "solo/ai-tts.flowgraph.toml",
        [node("ch_synth", "flowgraph.literal.string", "ai")]
        + [node(key, feature, value) for key, feature, value in (
            ("engine", "flowgraph.literal.string", "coeiroink"),
            ("voice", "flowgraph.literal.string", "fixture"),
            ("endpoint", "flowgraph.literal.string", "http://127.0.0.1:50032"),
            ("speed", "flowgraph.literal.float", 1.0),
            ("pitch", "flowgraph.literal.float", 0.0),
            ("volume", "flowgraph.literal.float", 1.0),
            ("lane", "flowgraph.literal.string", "assistant"),
            ("non_blocking", "flowgraph.literal.bool", True),
            ("lip_sync_base_key", "flowgraph.literal.string", "avatar.base"),
            ("lip_sync_target", "flowgraph.literal.string", "avatar"),
        )],
    )
    legacy("solo/translate.flowgraph.toml", [
        node("libre_base", "flowgraph.literal.string", "http://127.0.0.1:5000"),
        node("to_en", "flowgraph.literal.string", "en"),
        node("from_ja", "flowgraph.literal.string", "ja"),
    ])
    legacy("solo/twitch-chat.flowgraph.toml", [node("in", "flowgraph.ingress.twitch")])
    legacy("solo/ocr.flowgraph.toml", [
        node("trigger", "flowgraph.ingress.web_input"),
        node("title_regex", "flowgraph.literal.string", "fixture"),
        node("client_only", "flowgraph.literal.bool", True),
        node("lang", "flowgraph.literal.string", "ja"),
        node("ch_out", "flowgraph.literal.string", "ocr"),
    ])

    avatar_values = [
        node("unmfz_base_key", "flowgraph.literal.string", "avatar.base"),
        node("unmfz_topic_mode", "flowgraph.literal.string", "relative"),
        node("unmfz_schema_major", "flowgraph.literal.int", 1),
        node("dry_run", "flowgraph.literal.bool", False),
        node("playback_fps", "flowgraph.literal.float", 30.0),
    ]
    write(root / "avatar-reaction.flowgraph.toml", "\n".join([
        node("in", "flowgraph.ingress.channel_subscribe"), *avatar_values,
    ]))
    write(root / "avatar-pose.flowgraph.toml", node("in", "flowgraph.ingress.channel_subscribe"))

    points_nodes = [
        node("eventsub", "flowgraph.ingress.twitch_eventsub"),
        node("reward_id_path", "flowgraph.literal.string", "reward.id"),
        node("reward_id", "flowgraph.json.path"),
        node("kaltsit_reward_id", "flowgraph.literal.string", "fixture-reward"),
        node("is_kaltsit_request", "flowgraph.logic.equals"),
        node("route", "flowgraph.control.branch"),
        node("user_input_path", "flowgraph.literal.string", "user_input"),
        node("user_input", "flowgraph.json.path"),
        node("ai_request_prefix", "flowgraph.literal.string", "request: "),
        node("ai_request_content", "flowgraph.text.concat"),
        node("route_meta", "flowgraph.json.object"),
        node("emit_ai_request", "flowgraph.channel.emit"),
        node("avatar_intent", "flowgraph.json.object"),
        node("avatar_channel", "flowgraph.literal.string", "avatar"),
        node("avatar_intent_string", "flowgraph.json.stringify"),
        node("emit_avatar_reaction", "flowgraph.channel.emit"),
        node("effect_content", "flowgraph.literal.string", "effect"),
        node("effect_channel", "flowgraph.literal.string", "effect"),
        node("effect_flags", "flowgraph.json.object"),
        node("effect_meta", "flowgraph.json.object"),
        node("emit_effect", "flowgraph.channel.emit"),
        node("reward_title_path", "flowgraph.literal.string", "reward.title"),
        node("reward_title", "flowgraph.json.path"),
        node("observe_channel", "flowgraph.literal.string", "observe"),
        node("emit_observation", "flowgraph.channel.emit"),
    ]
    write(root / "twitch-channel-points/main.flowgraph.toml", "\n".join([
        'edges = []\n\n[meta]\ntitle = "fixture"', *points_nodes,
    ]))
    write(root / "keep.txt", "profile-owned sentinel")
    return profile


class ManagedProfileMigrationTests(unittest.TestCase):
    def run_migrator(self, profile: pathlib.Path, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(MIGRATOR), str(profile), *args],
            text=True,
            encoding="utf-8",
            capture_output=True,
            check=False,
        )

    def test_dry_run_does_not_change_profile_or_create_backup(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            profiles = pathlib.Path(temp) / "profiles"
            profile = make_legacy_profile(profiles)
            before = tree_hash(profile)
            result = self.run_migrator(profile, "--dry-run")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(json.loads(result.stdout)["dry_run"])
            self.assertEqual(tree_hash(profile), before)
            self.assertEqual(list(profiles.glob("fixture.backup-explicit-*")), [])
            self.assertEqual(list(profiles.glob(".fixture.flowgraph-staging-*")), [])

    def test_apply_preserves_profile_and_removes_public_channels(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            profiles = pathlib.Path(temp) / "profiles"
            profile = make_legacy_profile(profiles)
            result = self.run_migrator(profile)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertFalse(report["dry_run"])
            self.assertEqual((profile / "flowgraph.local/keep.txt").read_text(encoding="utf-8").strip(), "profile-owned sentinel")
            self.assertEqual(len(list(profiles.glob("fixture.backup-explicit-*"))), 1)
            for path in (profile / "flowgraph.local").rglob("*.flowgraph.toml"):
                doc = tomllib.loads(path.read_text(encoding="utf-8-sig"))
                features = [n.get("feature") for n in doc.get("nodes", [])]
                self.assertFalse(REMOVED.intersection(features), path)
                self.assertFalse(any(feature.startswith("flowgraph.library.") for feature in features), path)
                self.assertNotIn("library_uses", doc.get("meta", {}), path)
            self.assertTrue((profile / "flowgraph.local/translation-en/main.flowgraph.toml").is_file())
            self.assertTrue((profile / "flowgraph.local/assistant-output/main.flowgraph.toml").is_file())

    def test_validation_failure_leaves_active_tree_unchanged(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            profiles = pathlib.Path(temp) / "profiles"
            profile = make_legacy_profile(profiles)
            write(profile / "flowgraph.local/unexpected.flowgraph.toml", "\n".join([
                '[meta]\nlibrary_uses = []',
                node("unexpected", "flowgraph.library.input"),
            ]))
            before = tree_hash(profile)
            result = self.run_migrator(profile)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("active profile was not changed", result.stderr)
            self.assertEqual(tree_hash(profile), before)
            self.assertEqual(list(profiles.glob(".fixture.flowgraph-staging-*")), [])
            self.assertEqual(list(profiles.glob(".fixture.flowgraph-swap-*")), [])


if __name__ == "__main__":
    unittest.main()
