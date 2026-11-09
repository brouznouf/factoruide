"""Protocol checks requiring no game data, compiler, or planner processes."""
import unittest
import tempfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, patch
import route_quality_compare as compare


class ProtocolTests(unittest.TestCase):
    def test_tuning_is_balanced_and_has_64_unique_runs(self):
        runs = compare.schedule("tuning")
        self.assertEqual(len(runs), 64)
        self.assertEqual(len({(r['profile'], r['seed'], r['arm']) for r in runs}), 64)
        for arm in compare.ARMS:
            self.assertEqual([r['arm'] for r in runs].count(arm), 16)

    def test_heldout_requires_frozen_candidate_and_has_32_runs(self):
        with self.assertRaises(ValueError):
            compare.schedule("heldout")
        runs = compare.schedule("heldout", "chain")
        self.assertEqual(len(runs), 32)
        self.assertEqual([r['arm'] for r in runs[:4]], ['baseline', 'chain', 'chain', 'baseline'])

    def test_profile_weight_is_equal_not_seed_weight(self):
        pairs = [{'profile': 'a', 'gain_ratio': 0.98, 'play_ratio': 1.0}] * 2
        pairs += [{'profile': 'b', 'gain_ratio': 1.02, 'play_ratio': 1.0}]
        self.assertAlmostEqual(compare.aggregate(pairs)['score_ratio'], (0.98 * 1.02) ** 0.5)

    def test_win_gate_rejects_resource_or_validity_failure(self):
        pairs = [{'profile': str(p), 'gain_ratio': 0.98, 'play_ratio': 1.0,
                  'score_delta': 100, 'new_failure': p == 0} for p in range(8)]
        self.assertFalse(compare.aggregate(pairs)['practical_win'])

    def test_win_gate_requires_six_profiles_and_worst_regression_limit(self):
        pairs = [{'profile': str(p), 'gain_ratio': 0.98, 'play_ratio': 1.0,
                  'score_delta': 100, 'new_failure': False} for p in range(8)]
        self.assertTrue(compare.aggregate(pairs)['practical_win'])
        pairs[0]['gain_ratio'] = 1.021
        self.assertFalse(compare.aggregate(pairs)['practical_win'])

    def test_compute_and_memory_violations_remain_failures(self):
        row = {'returncode': 0, 'memory_observed': True, 'peak_memory_bytes': compare.MEMORY_LIMIT + 1,
               'result': {'valid': True, 'required_validation': 'passed', 'compute_seconds': 20.251,
                          'budget_seconds': 20, 'score': 100, 'player_seconds': 90}}
        self.assertEqual(compare.failures(row), ['compute_budget', 'memory_limit'])

    def test_missing_replicate_cannot_establish_win(self):
        self.assertFalse(compare.analysis([], 'heldout', 'chain')['metrics']['practical_win'])
        self.assertEqual(compare.analysis([], 'heldout', 'chain')['metrics']['usable_pairs'], 0)

    def test_corrupt_output_becomes_recorded_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'result.json'
            path.write_text('{broken', encoding='utf-8')
            self.assertIsNone(compare.read_artifact(path))

    def test_interrupted_run_is_preserved_without_selective_retry(self):
        with tempfile.TemporaryDirectory() as tmp:
            args = SimpleNamespace(output=Path(tmp), phase='pilot', budget=20, reserve=3, binary=Path('unused'))
            run = compare.schedule('pilot')[0]
            identity = f"pilot-000-{run['profile']}-{run['seed']}-{run['arm']}"
            (args.output / identity).mkdir()
            with patch.object(compare.subprocess, 'Popen', side_effect=AssertionError('must not retry')):
                compare.run_one(args, run, 0)
            record = compare.records(args.output)[0]
            self.assertEqual(record['killed'], 'interrupted_or_unrecorded_prior_attempt')
            self.assertIn('missing_result', record['failures'])

    def warning_pair(self, baseline_id, candidate_id, category='missing_class'):
        rows = []
        for arm, quest_id, reason in [('baseline', baseline_id, 'prerequisite missing'),
                                     ('chain', candidate_id, 'only after target level')]:
            rows.append({'phase': 'tuning', 'profile': compare.PROFILES[0]['id'], 'seed': 42, 'arm': arm,
                         'returncode': 0, 'memory_observed': True, 'peak_memory_bytes': 100,
                         'result': {'valid': True, 'required_validation': 'passed', 'compute_seconds': 20,
                                    'budget_seconds': 20, 'score': 100, 'player_seconds': 90, 'breakdown': {},
                                    'warning_quest_ids': {category: [quest_id]},
                                    'warnings': {category: [f'Quest ({quest_id}): {reason}']}}})
        return compare.paired(rows, 'tuning', 'chain')[0]

    def test_same_missing_quest_changed_explanation_is_not_new_failure(self):
        for category in ('missing_class', 'missing_zone', 'late_timed'):
            with self.subTest(category=category):
                self.assertFalse(self.warning_pair(101, 101, category)['new_failure'])

    def test_replacement_missing_quest_is_failure_even_when_count_equal(self):
        self.assertTrue(self.warning_pair(101, 102)['new_failure'])

    def test_warning_without_stable_identity_fails_closed(self):
        row = {'returncode': 0, 'memory_observed': True, 'peak_memory_bytes': 100,
               'result': {'valid': True, 'required_validation': 'passed', 'compute_seconds': 20,
                          'budget_seconds': 20, 'score': 100, 'player_seconds': 90,
                          'warnings': {'missing_class': ['Quest (101): reason']}}}
        self.assertIn('warning_identity_missing', compare.failures(row))

    def test_pair_references_retain_metrics_without_repeating_canonical_traces(self):
        row = {'id': 'run-1', 'artifact': 'run-1/result.json', 'peak_memory_bytes': 100,
               'result': {'score': 100, 'player_seconds': 90, 'stages_seconds': {'export': .1},
                          'warning_quest_ids': {'missing_class': [101]}, 'trace': ['large'],
                          'guide': {'steps': ['large']}, 'anytime': ['large']}}
        summary = compare.run_summary(row)
        self.assertEqual(summary['id'], row['id'])
        self.assertEqual(summary['artifact'], row['artifact'])
        self.assertEqual(summary['result']['score'], 100)
        self.assertEqual(summary['result']['stages_seconds'], {'export': .1})
        self.assertEqual(summary['result']['warning_quest_ids'], {'missing_class': [101]})
        self.assertTrue({'trace', 'guide', 'anytime'}.isdisjoint(summary['result']))

    def test_exceptional_exit_kills_and_reaps_before_lock_release(self):
        for origin, exception in [('observer', KeyboardInterrupt()), ('observer', RuntimeError('observer failed')),
                                  ('poll', RuntimeError('poll failed'))]:
            with self.subTest(exception=type(exception).__name__, origin=origin), tempfile.TemporaryDirectory() as tmp:
                args = SimpleNamespace(output=Path(tmp), phase='pilot', budget=20, reserve=3, binary=Path('unused'))
                run = compare.schedule('pilot')[0]
                events = []
                process = Mock(returncode=-9)
                process.poll.return_value = None
                process.kill.side_effect = lambda: events.append('kill')
                process.wait.side_effect = lambda: events.append('wait')
                observer = Mock(peak=100, available=True)
                if origin == 'observer':
                    observer.sample.side_effect = exception
                else:
                    process.poll.side_effect = exception

                @compare.contextlib.contextmanager
                def lock():
                    try:
                        yield
                    finally:
                        events.append('unlock')

                with patch.object(compare, 'experiment_lock', lock), \
                     patch.object(compare, 'MemoryObserver', return_value=observer), \
                     patch.object(compare.subprocess, 'Popen', return_value=process) as popen:
                    with self.assertRaises(type(exception)), compare.experiment_lock():
                        compare.run_one(args, run, 0)
                self.assertEqual(events, ['kill', 'wait', 'unlock'])
                self.assertEqual(popen.call_count, 1)
                saved = compare.records(args.output)
                self.assertEqual(len(saved), 1)
                self.assertEqual(saved[0]['killed'], 'exceptional_exit')
                self.assertEqual(saved[0]['exception_type'], type(exception).__name__)


if __name__ == '__main__':
    unittest.main()
