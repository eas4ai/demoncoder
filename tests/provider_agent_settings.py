#!/usr/bin/env python3
"""Exercise provider checks and role choices through the production terminal."""
import json
from pathlib import Path
import tempfile
import time
import tomllib
import unittest
import sys
sys.dont_write_bytecode = True
from onboarding import App, CatalogServer, DOWN, END, ENTER, ESCAPE, HOME


def api_config(server, key=None):
    text = '[settings]\nproviders=[]\n[connections.openai-api]\nadapter="openai-api"\n'
    text += "endpoint=" + json.dumps(server.endpoint()) + "\n"
    if key is not None:
        text += "api_key=" + json.dumps(key) + "\n"
    return text


def providers(app):
    app.answer("Trust this project for coding tools? [N]:", "y")
    app.wait_current("Space toggles")
    return app.wait_current("Assignments grant no permissions.")


def choose_provider(app, index):
    app.send(HOME + DOWN * index + b" ")


def cancel_setup(app):
    app.send(b"\x11")
    app.process.wait(timeout=5)
    assert app.process.returncode != 0
    assert not app.log.exists()


def backend_requests(app):
    return [json.loads(line) for line in app.backend_requests.read_text().splitlines()] if app.backend_requests.exists() else []

class ProviderSettings(unittest.TestCase):
    def test_provider_checkboxes_precede_models(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-") as directory:
            app = App(Path(directory))
            try:
                screen = providers(app)
                self.assertEqual(screen.count("[ ]"), 4)
                self.assertNotIn("Creator model", screen)
                app.send(END + ENTER)
                screen = app.wait_current("Select at least one provider.")
                self.assertNotIn("Enter assigns", screen)
                cancel_setup(app)
                self.assertFalse(app.settings.exists())
            finally:
                app.close()

    def test_cli_presence_does_not_establish_subscription_login(self):
        cases = [
            ({}, (), "install codex"),
            ({"codex": {"logged_in": False}}, ("codex",), "requires a ChatGPT subscription login"),
            ({"codex": {"account_type": "apiKey"}}, ("codex",), "requires a ChatGPT subscription login"),
        ]
        for profile, installed, recovery in cases:
            with self.subTest(profile=profile), tempfile.TemporaryDirectory(prefix="demoncoder-settings-login-") as directory:
                app = App(Path(directory), backend_profiles=profile, backends=installed)
                try:
                    providers(app)
                    choose_provider(app, 2)
                    screen = app.wait_current(recovery)
                    self.assertIn("Unavailable", screen)
                    self.assertNotIn("Authenticated", screen)
                    app.send(END + ENTER)
                    screen = app.wait_current("Check each selected provider")
                    self.assertNotIn("Enter assigns", screen)
                    requests = backend_requests(app)
                    self.assertFalse(any(request["message"].get("method") in ("model/list", "thread/start", "turn/start") for request in requests))
                    cancel_setup(app)
                    self.assertFalse(app.settings.exists())
                finally:
                    app.close()

    def test_missing_key_and_rejected_key_never_appear_authenticated(self):
        server = CatalogServer()
        try:
            for key in (None, "synthetic-rejected-key"):
                with self.subTest(key=key), tempfile.TemporaryDirectory(prefix="demoncoder-settings-key-") as directory:
                    app = App(Path(directory), preconfig=api_config(server, key))
                    before = app.settings.read_bytes()
                    request_count = len(server.requests)
                    try:
                        providers(app)
                        choose_provider(app, 3)
                        if key is None:
                            app.wait_current("API key: Enter checks")
                            app.send(ENTER)
                            screen = app.wait_current("set OPENAI_API_KEY")
                            self.assertEqual(len(server.requests), request_count)
                        else:
                            screen = app.wait_current("HTTP 401")
                            self.assertEqual(server.requests[-1]["authorization"], "Bearer " + key)
                            self.assertNotIn(key.encode(), app.output)
                        self.assertIn("Unavailable", screen)
                        self.assertNotIn("Authenticated", screen)
                        self.assertNotIn("Enter assigns", screen)
                        cancel_setup(app)
                        self.assertEqual(app.settings.read_bytes(), before)
                    finally:
                        app.close()
        finally:
            server.close()

    def test_correcting_a_rejected_key_offers_only_checked_models(self):
        server = CatalogServer()
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-key-repair-") as directory:
            app = App(Path(directory), preconfig=api_config(server, "synthetic-rejected-key"))
            try:
                providers(app)
                choose_provider(app, 3)
                app.wait_current("HTTP 401")
                app.send(b"k")
                app.wait_current("API key: Enter checks")
                app.send("synthetic-onboarding-secret\r")
                app.wait_current("Authenticated")
                app.send(END + ENTER)
                screen = app.wait_current("Enter assigns")
                self.assertIn("fixture-model · OpenAI API (openai-api)", screen)
                self.assertNotIn("Codex subscription", screen)
                app.send(END + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["settings"]["providers"], ["openai-api"])
                self.assertEqual(settings["connections"]["openai-api"]["api_key"], "synthetic-onboarding-secret")
                app.send("local-selection\r")
                app.wait("RECEIVED-local-selection")
                posted = [request for request in server.requests if request["method"] == "POST"]
                self.assertEqual(len(posted), 1)
                self.assertEqual(posted[0]["body"]["model"], "fixture-model")
                self.assertEqual(posted[0]["authorization"], "Bearer synthetic-onboarding-secret")
                self.assertNotIn("synthetic-onboarding-secret", app.log.read_text())
                self.assertNotIn(b"synthetic-onboarding-secret", app.output)
                app.finish()
            finally:
                app.close()
                server.close()

    def test_slow_check_can_be_cancelled_while_other_provider_completes(self):
        server = CatalogServer({"/openai": {"key": "synthetic-slow-key", "models": ["must-not-be-offered"], "delay": 20}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-cancel-") as directory:
            app = App(Path(directory), preconfig=api_config(server, "synthetic-slow-key"))
            try:
                providers(app)
                choose_provider(app, 3)
                app.wait_current("Checking")
                choose_provider(app, 2)
                app.wait_current("Codex subscription (codex) — Authenticated")
                app.send(END + ENTER)
                screen = app.wait_current("Check each selected provider")
                self.assertNotIn("Enter assigns", screen)
                app.send(ESCAPE)
                app.wait_current("Cancelled · press r to retry")
                choose_provider(app, 3)
                app.wait_current("OpenAI API (openai-api) — Not selected")
                app.send(END + ENTER)
                screen = app.wait_current("Enter assigns")
                self.assertNotIn("must-not-be-offered", screen)
                self.assertNotIn("OpenAI API", screen)
                app.send(END + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["settings"]["providers"], ["codex"])
                self.assertFalse(any(request["method"] == "POST" for request in server.requests))
                self.assertTrue(all(request["message"].get("method") in ("initialize", "initialized", "config/read", "account/read", "model/list") for request in backend_requests(app)))
                app.finish()
            finally:
                app.close()
                server.close()

    def test_slow_subscription_check_times_out_without_authentication(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-timeout-") as directory:
            app = App(Path(directory), backend_profiles={"codex": {"delay": 20}})
            try:
                providers(app)
                start = time.monotonic()
                choose_provider(app, 2)
                app.wait_current("Checking")
                screen = app.wait_current("provider check timed out", timeout=18)
                self.assertLess(time.monotonic() - start, 18)
                self.assertIn("Unavailable", screen)
                self.assertNotIn("Authenticated", screen)
                self.assertFalse(any(request["message"].get("method") == "model/list" for request in backend_requests(app)))
                cancel_setup(app)
                self.assertFalse(app.settings.exists())
            finally:
                app.close()

    def test_overlapping_model_ids_keep_provider_identity_and_request_route(self):
        server = CatalogServer()
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-identity-") as directory:
            app = App(Path(directory), preconfig=api_config(server, "synthetic-onboarding-secret"))
            try:
                providers(app)
                choose_provider(app, 2)
                choose_provider(app, 3)
                app.wait_current("Codex subscription (codex) — Authenticated")
                app.wait_current("OpenAI API (openai-api) — Authenticated")
                app.send(END + ENTER)
                screen = app.wait_current("Enter assigns")
                self.assertIn("fixture-model · Codex subscription (codex)", screen)
                self.assertIn("fixture-model · OpenAI API (openai-api)", screen)
                self.assertNotIn("Anthropic API", screen)
                self.assertNotIn("Claude subscription", screen)
                app.send(END + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["settings"]["creator"]["connection"], "openai-api")
                self.assertEqual(settings["settings"]["creator"]["model"], "fixture-model")
                app.send("route-identity\r")
                app.wait("RECEIVED-route-identity")
                posted = [request for request in server.requests if request["method"] == "POST"]
                self.assertEqual(posted[-1]["path"], "/openai/responses")
                self.assertEqual(posted[-1]["body"]["model"], "fixture-model")
                self.assertFalse(any(request["message"].get("method") == "turn/start" for request in backend_requests(app)))
                app.finish()
            finally:
                app.close()
                server.close()

    def test_claude_status_is_a_managed_login_observation_and_aliases_are_labelled(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-claude-") as directory:
            app = App(Path(directory), environment={"OPENAI_API_KEY": "synthetic-unselected-openai", "ANTHROPIC_API_KEY": "synthetic-unselected-anthropic", "CLAUDE_CONFIG_DIR": str(Path(directory) / "must-not-use"), "CLAUDE_CODE_OAUTH_TOKEN": "synthetic-token-override"})
            try:
                providers(app)
                choose_provider(app, 1)
                screen = app.wait_current("Claude reports a managed subscription login")
                self.assertIn("supported backend aliases", screen)
                app.send(END + ENTER)
                screen = app.wait_current("Enter assigns")
                for model in ("haiku", "opus", "sonnet"):
                    self.assertIn(model + " · Claude subscription (claude)", screen)
                self.assertEqual([request["message"]["method"] for request in backend_requests(app)], ["auth/status"])
                cancel_setup(app)
                self.assertFalse(app.settings.exists())
            finally:
                app.close()

    def test_environment_key_controls_checks_without_replacing_saved_key(self):
        server = CatalogServer({"/openai": {"key": "synthetic-environment-key", "models": ["fixture-model"]}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-env-key-") as directory:
            app = App(Path(directory), preconfig=api_config(server, "synthetic-saved-key"), environment={"OPENAI_API_KEY": "synthetic-environment-key"})
            try:
                providers(app)
                choose_provider(app, 3)
                app.wait_current("Authenticated")
                self.assertEqual(server.requests[0]["authorization"], "Bearer synthetic-environment-key")
                app.send(END + ENTER)
                app.wait_current("Enter assigns")
                app.send(ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["connections"]["openai-api"]["api_key"], "synthetic-saved-key")
                self.assertNotIn(b"synthetic-environment-key", app.output)
                self.assertNotIn("synthetic-environment-key", app.log.read_text())
                app.finish()
            finally:
                app.close()
                server.close()

    def test_reflected_credential_is_not_rendered_as_a_model(self):
        key = "synthetic-catalog-secret"
        server = CatalogServer({"/openai": {"key": key, "models": ["reflected-" + key]}})
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-catalog-secret-") as directory:
            app = App(Path(directory), preconfig=api_config(server, key))
            before = app.settings.read_bytes()
            try:
                providers(app)
                choose_provider(app, 3)
                screen = app.wait_current("credential material")
                self.assertIn("Unavailable", screen)
                self.assertNotIn("Authenticated", screen)
                self.assertNotIn(key.encode(), app.output)
                cancel_setup(app)
                self.assertEqual(app.settings.read_bytes(), before)
            finally:
                app.close()
                server.close()

    def test_failed_model_discovery_cannot_contribute_choices(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-no-catalog-") as directory:
            app = App(Path(directory), backend_profiles={"codex": {"catalog_error": True}})
            try:
                providers(app)
                choose_provider(app, 2)
                screen = app.wait_current("Unavailable")
                self.assertNotIn("Authenticated", screen)
                app.send(END + ENTER)
                screen = app.wait_current("Check each selected provider")
                self.assertNotIn("Enter assigns", screen)
                self.assertNotIn("fixture-model", screen)
                cancel_setup(app)
                self.assertFalse(app.settings.exists())
            finally:
                app.close()

    def test_saved_unavailable_model_requires_an_explicit_replacement(self):
        preconfig = '[connections.codex]\nadapter="codex"\nmodel="retired-model"\n[settings]\nproviders=[]\n[settings.creator]\nconnection="codex"\nmodel="retired-model"\n'
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-retired-model-") as directory:
            app = App(Path(directory), preconfig=preconfig)
            before = app.settings.read_bytes()
            try:
                providers(app)
                choose_provider(app, 2)
                app.wait_current("Authenticated")
                app.send(END + ENTER)
                app.wait_current("Settings · Agent assignments")
                app.send(END + ENTER)
                app.wait_current("Creator model is unavailable in the checked catalog")
                self.assertEqual(app.settings.read_bytes(), before)
                self.assertFalse(app.log.exists())
                app.send(HOME + ENTER)
                screen = app.wait_current("Enter assigns")
                self.assertNotIn("retired-model", screen)
                app.send(END + ENTER)
                app.wait_current("Creator — fixture-model")
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())
                self.assertEqual(settings["settings"]["creator"]["model"], "fixture-model")
                app.finish()
            finally:
                app.close()

    def test_roles_default_override_cancel_and_creator_change_are_distinct(self):
        with tempfile.TemporaryDirectory(prefix="demoncoder-settings-roles-") as directory:
            app = App(Path(directory))
            try:
                providers(app)
                choose_provider(app, 2)
                app.wait_current("Authenticated")
                app.send(END + ENTER)
                app.wait_current("Enter assigns")
                app.send(END + ENTER)
                screen = app.wait_current("Settings · Agent assignments")
                for role in ("Creator", "Worker", "Oracle", "Reviewer", "Advisor", "Judge"):
                    self.assertIn(role + " — ", screen)
                self.assertEqual(screen.count("Use Creator model → fixture-model"), 5)
                self.assertIn("Outside-access decisions", screen)
                self.assertIn("Advice on delegated work", screen)

                app.send(HOME + DOWN * 3 + ENTER)
                app.wait_current("Enter assigns")
                app.send(END + ENTER)
                app.wait_current("Reviewer — fixture-model")
                app.send(HOME + ENTER)
                screen = app.wait_current("Enter assigns")
                self.assertIn("✓ fixture-model", screen)
                app.send(HOME + ENTER)
                screen = app.wait_current("Creator — changed-model")
                self.assertIn("Reviewer — fixture-model", screen)
                self.assertEqual(screen.count("Use Creator model → changed-model"), 4)

                app.send(HOME + DOWN + ENTER)
                app.wait_current("Enter assigns")
                app.send(END + ESCAPE)
                screen = app.wait_current("Settings · Agent assignments")
                self.assertIn("Worker — Use Creator model → changed-model", screen)
                app.send(END + ENTER)
                app.wait("Prompt")
                settings = tomllib.loads(app.settings.read_text())["settings"]
                self.assertEqual(settings["creator"]["model"], "changed-model")
                self.assertEqual(set(settings["overrides"]), {"reviewer"})
                self.assertEqual(settings["overrides"]["reviewer"]["model"], "fixture-model")
                app.send("creator-after-change\r")
                app.wait("RECEIVED-creator-after-change")
                threads = [request["message"] for request in backend_requests(app) if request["message"].get("method") == "thread/start"]
                self.assertEqual(threads[-1]["params"]["model"], "changed-model")
                app.finish()
            finally:
                app.close()


if __name__ == "__main__":
    unittest.main(verbosity=2)
