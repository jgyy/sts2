import unittest
from types import SimpleNamespace
from unittest import mock

from sts2bot import __main__ as cli
from sts2bot import service


class ServiceCommands(unittest.TestCase):
    def setUp(self):
        # Never let a test reach the real systemctl: that would stop the live bot.
        self.enterContext(mock.patch("sts2bot.service.subprocess.run", side_effect=AssertionError("tests must not run systemctl")))
        self.printed = self.enterContext(mock.patch("builtins.print"))

    def test_disable_stops_the_unit_and_keeps_it_from_starting_at_login(self):
        calls = []
        self.assertEqual(service.main(["disable"], runner=lambda args: calls.append(args) or SimpleNamespace(returncode=0)), 0)
        self.assertEqual(calls, [["systemctl", "--user", "disable", "--now", "sts2bot"]])

    def test_each_action_maps_to_its_systemctl_command(self):
        expected = {"status": ["systemctl", "--user", "status", "--no-pager", "--lines=0", "sts2bot"], "start": ["systemctl", "--user", "start", "sts2bot"],
                    "stop": ["systemctl", "--user", "stop", "sts2bot"], "enable": ["systemctl", "--user", "enable", "--now", "sts2bot"],
                    "disable": ["systemctl", "--user", "disable", "--now", "sts2bot"]}
        for action, args in expected.items():
            self.assertEqual(service.systemctl_args(action), args)

    def test_a_failing_systemctl_gives_its_exit_code_and_no_success_note(self):
        self.assertEqual(service.main(["disable"], runner=lambda args: SimpleNamespace(returncode=5)), 5)
        self.printed.assert_not_called()

    def test_unknown_action_is_refused(self):
        with self.assertRaises(SystemExit):
            service.main(["restart"], runner=lambda args: SimpleNamespace(returncode=0))

    def test_the_cli_sends_service_commands_to_the_service_not_the_bot(self):
        with mock.patch("sts2bot.service.subprocess.run", return_value=SimpleNamespace(returncode=0)) as run:
            self.assertEqual(cli.main(["service", "disable"]), 0)
        run.assert_called_once_with(["systemctl", "--user", "disable", "--now", "sts2bot"])

    def test_a_missing_systemctl_says_so(self):
        with mock.patch("sys.stderr"):
            self.assertEqual(service.main(["stop"], runner=mock.Mock(side_effect=FileNotFoundError("systemctl"))), 127)
