import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))

import worker


class WorkerTests(unittest.TestCase):
    def test_normalize_params_flattens_single_list(self):
        params = [["Lunu User"]]
        self.assertEqual(worker.normalize_params(params), ["Lunu User"])

    def test_normalize_params_keeps_plain(self):
        params = ["Lunu User"]
        self.assertEqual(worker.normalize_params(params), ["Lunu User"])

    def test_handle_echo_string(self):
        result = worker.handle("echo", [["Lunu User"]])
        self.assertEqual(result["result"], "Lunu User")

    def test_handle_echo_boolean(self):
        result = worker.handle("echo", [[True]])
        self.assertIs(result["result"], True)

    def test_handle_hello(self):
        result = worker.handle("hello", [["Lunu User"]])
        self.assertEqual(result["result"], "Hello from Python")


if __name__ == "__main__":
    unittest.main()
