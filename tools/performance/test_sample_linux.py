import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from sample_linux import cpu_delta, percentile, process_tree, read_process, read_pss


def process(pid, ppid=0, start=100, ticks=10):
    return dict(pid=pid, ppid=ppid, start=start, ticks=ticks, rss=4096)


class SamplingTests(unittest.TestCase):
    def test_stat_command_with_parentheses_and_spaces(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "123"
            path.mkdir()
            fields = ["S", "1"] + ["0"] * 20
            fields[11], fields[12] = "5", "7"
            fields[19], fields[21] = "900", "3"
            (path / "stat").write_text("123 (shell (worker)) " + " ".join(fields))
            with patch("sample_linux.os.sysconf", return_value=4096):
                self.assertEqual(
                    read_process(path), process(123, 1, 900, 12) | {"rss": 12288}
                )

    def test_cpu_does_not_count_pid_reuse_or_new_process_lifetime(self):
        previous = [process(1, ticks=20), process(2, ticks=100)]
        current = [
            process(1, ticks=70),
            process(2, start=200, ticks=800),
            process(3, ticks=90),
        ]
        self.assertEqual(cpu_delta(previous, current, 2, 100), 25)

    def test_recursive_tree_excludes_unrelated_processes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            entries = {1: process(1), 2: process(2, 1), 3: process(3, 2), 4: process(4)}
            for pid in entries:
                (root / str(pid)).mkdir()
            with patch(
                "sample_linux.read_process", side_effect=lambda p: entries[int(p.name)]
            ):
                self.assertEqual(
                    [p["pid"] for p in process_tree(1, 100, root)], [1, 2, 3]
                )
                with self.assertRaisesRegex(RuntimeError, "reused"):
                    process_tree(1, 101, root)

    def test_pss_unavailable_and_pid_reuse_are_not_zero_memory(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            self.assertIsNone(read_pss(path, 100))
            (path / "smaps_rollup").write_text(
                "Pss:                123 kB\nPss_Dirty: 99 kB\n"
            )
            with patch("sample_linux.read_process", return_value={"start": 100}):
                self.assertEqual(read_pss(path, 100), 123 * 1024)
            with patch("sample_linux.read_process", return_value={"start": 101}):
                self.assertIsNone(read_pss(path, 100))

    def test_nearest_rank_p95(self):
        self.assertEqual(percentile(list(range(1, 101)), 0.95), 95)
        with self.assertRaises(ValueError):
            percentile([], 0.95)


if __name__ == "__main__":
    unittest.main()
