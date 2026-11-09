#!/usr/bin/env python3
"""Sequential single-start kernel experiment (NOT the production tournament).

No third-party packages or build commands. Workflow:
  run --phase pilot; freeze-protocol; run --phase tuning;
  freeze-candidate; run --phase heldout; analyze
Pass the same --output directory to each command. Every run is a fresh process.
"""
import argparse
import contextlib
import ctypes
import csv
import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
ARMS = ('baseline', 'chain', 'hub', 'exact')
MEMORY_LIMIT = 1536 * 1024 * 1024
PROFILES = [
    {'id': f'{edition}-{race}-{class_}-{lo}-{hi}', 'edition': edition,
     'request': {'race': race, 'class': class_, 'from_level': lo, 'to_level': hi}}
    for edition in ('classic', 'forever')
    for race, class_, lo, hi in [('human', 'warrior', 1, 30), ('undead', 'mage', 1, 40),
                                ('orc', 'warrior', 20, 40), ('gnome', 'mage', 20, 40)]
]


def schedule(phase, candidate=None):
    if phase == 'heldout' and candidate not in ARMS[1:]:
        raise ValueError('heldout requires one frozen nonbaseline candidate')
    profiles = [PROFILES[0], PROFILES[5]] if phase == 'pilot' else PROFILES
    seeds = (20261107, 314159) if phase == 'heldout' else (42, 1337)
    if phase == 'pilot':
        seeds = (42,)
    runs = []
    for p, profile in enumerate(profiles):
        for s, seed in enumerate(seeds):
            if phase == 'heldout':
                arms = ('baseline', candidate) if (p + s) % 2 == 0 else (candidate, 'baseline')
            else:
                # Cyclic Latin square, with reversed second replicate to counterbalance order.
                shift = (p + s) % len(ARMS)
                arms = ARMS[shift:] + ARMS[:shift]
                if s:
                    arms = tuple(reversed(arms))
            runs.extend({'profile': profile['id'], 'edition': profile['edition'],
                         'request': profile['request'], 'seed': seed, 'arm': arm} for arm in arms)
    return runs


def geometric(values):
    values = list(values)
    return math.exp(statistics.mean(math.log(v) for v in values)) if values else None


def aggregate(pairs):
    grouped = {}
    for pair in pairs:
        grouped.setdefault(pair['profile'], []).append(pair)
    score_ratio = geometric(geometric(p['gain_ratio'] for p in ps) for ps in grouped.values())
    play_ratio = geometric(geometric(p['play_ratio'] for p in ps) for ps in grouped.values())
    positive_profiles = sum(statistics.mean(p.get('score_delta', 1 - p['gain_ratio']) for p in ps) > 0 for ps in grouped.values())
    worst = max((p['gain_ratio'] - 1 for p in pairs), default=math.inf)
    new_failure = any(p.get('new_failure', False) for p in pairs)
    deltas = [p.get('score_delta', 0) for p in pairs]
    return {'score_ratio': score_ratio, 'score_improvement': None if score_ratio is None else 1 - score_ratio,
            'player_ratio': play_ratio, 'positive_profiles': positive_profiles, 'profile_count': len(grouped),
            'worst_score_regression': worst if math.isfinite(worst) else None,
            'new_failure': new_failure,
            'win_tie_loss': {'win': sum(d > 0.5 for d in deltas), 'tie': sum(abs(d) <= 0.5 for d in deltas),
                             'loss': sum(d < -0.5 for d in deltas)},
            'median_score_delta': statistics.median(deltas) if deltas else None,
            'practical_win': bool(len(grouped) == 8 and score_ratio <= 0.99 and positive_profiles >= 6
                                  and worst <= 0.02 and play_ratio <= 1.0025 and not new_failure)}


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def source_identity(binary):
    # Include dirty contents and untracked implementation files, not just the HEAD revision.
    tracked = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT).decode().split('\0')
    paths = {Path(p) for p in tracked if p and (p.startswith(('crates/', 'overrides/', 'scripts/'))
                                                or p in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'))}
    for directory in ('crates/fg-route/src', 'crates/fg-route/examples', 'overrides'):
        paths.update(p.relative_to(ROOT) for p in (ROOT / directory).rglob('*') if p.is_file())
    paths.update(Path('scripts') / n for n in ('route_quality_compare.py', 'test_route_quality_compare.py'))
    files = {p.as_posix(): sha(ROOT / p) if (ROOT / p).is_file() else 'DELETED' for p in sorted(paths)}
    data = {e: sha(ROOT / 'data' / f'{e}.sqlite') for e in ('classic', 'forever')}
    return {'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
            'source_files': files, 'data_sha256': data, 'binary_sha256': sha(binary),
            'requests': PROFILES, 'runner_sha256': sha(__file__)}


def write_new(path, value):
    with Path(path).open('x', encoding='utf-8', newline='\n') as stream:
        json.dump(value, stream, sort_keys=True, indent=2, allow_nan=False)
        stream.write('\n')


def read(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))


def read_artifact(path):
    try:
        return read(path)
    except (OSError, ValueError):
        return None


def append_record(output, row):
    row['failures'] = failures(row)
    with (output / 'runs.jsonl').open('a', encoding='utf-8', newline='\n') as stream:
        stream.write(json.dumps(row, sort_keys=True, allow_nan=False) + '\n')
        stream.flush()
        os.fsync(stream.fileno())


def records(output):
    path = output / 'runs.jsonl'
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines() if line.strip()]


@contextlib.contextmanager
def experiment_lock():
    name = 'fg-quality-' + hashlib.sha256(str(ROOT).encode()).hexdigest()[:16] + '.lock'
    path = Path(tempfile.gettempdir()) / name
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError as error:
        raise RuntimeError(f'another runner holds {path}; remove only after verifying that it stopped') from error
    try:
        os.write(fd, str(os.getpid()).encode())
        os.close(fd)
        yield
    finally:
        path.unlink()


class MemoryObserver:
    def __init__(self):
        self.peak = 0
        self.available = False
        if os.name == 'nt':
            from ctypes import wintypes

            class Counters(ctypes.Structure):
                _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD)] + [
                    (name, ctypes.c_size_t) for name in ('PeakWorkingSetSize', 'WorkingSetSize',
                    'QuotaPeakPagedPoolUsage', 'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage',
                    'QuotaNonPagedPoolUsage', 'PagefileUsage', 'PeakPagefileUsage')]
            self.counter_type = Counters
            self.get_memory = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
            self.get_memory.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
            self.get_memory.restype = wintypes.BOOL

    def sample(self, process):
        if os.name == 'nt':
            counters = self.counter_type()
            counters.cb = ctypes.sizeof(counters)
            if self.get_memory(int(process._handle), ctypes.byref(counters), counters.cb):
                self.peak = max(self.peak, counters.PeakWorkingSetSize)
                self.available = True
        elif sys.platform.startswith('linux'):
            try:
                for line in Path(f'/proc/{process.pid}/status').read_text().splitlines():
                    if line.startswith('VmHWM:'):
                        self.peak = max(self.peak, int(line.split()[1]) * 1024)
                        self.available = True
            except FileNotFoundError:
                pass


def run_one(args, run, ordinal):
    identity = f"{args.phase}-{ordinal:03d}-{run['profile']}-{run['seed']}-{run['arm']}"
    folder = args.output / identity
    if folder.exists():
        # Never silently rerun a crashed or unrecorded attempt: preserve it as a failure.
        result_path = folder / 'result.json'
        append_record(args.output, {**run, 'id': identity, 'phase': args.phase, 'ordinal': ordinal,
                      'returncode': None, 'memory_observed': False, 'peak_memory_bytes': 0,
                      'killed': 'interrupted_or_unrecorded_prior_attempt',
                      'artifact': str(result_path.relative_to(args.output)), 'result': read_artifact(result_path)})
        return
    folder.mkdir(exist_ok=False)
    request = folder / 'request.json'
    write_new(request, run['request'])
    result_path = folder / 'result.json'
    command = [str(args.binary), '--edition', run['edition'], '--seed', str(run['seed']),
               '--variant', run['arm'], '--budget', str(args.budget), '--reserve', str(args.reserve),
               '--repo', str(ROOT), '--request', str(request), '--output', str(result_path)]
    started = time.perf_counter()
    memory = MemoryObserver()
    killed = None
    exceptional_exit = None
    with (folder / 'process.log').open('wb') as log:
        process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT,
                                   creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        try:
            while True:
                memory.sample(process)
                if memory.peak > MEMORY_LIMIT:
                    killed = 'memory_limit'
                elif time.perf_counter() - started > args.budget + 10:
                    killed = 'parent_timeout'
                if killed:
                    process.kill()
                    break
                if process.poll() is not None:
                    memory.sample(process)
                    break
                time.sleep(0.025)
        except BaseException as error:
            # KeyboardInterrupt inherits BaseException. Preserve it only after killing,
            # reaping and recording this attempt, while the caller still holds the lock.
            exceptional_exit = error
            killed = 'exceptional_exit'
        finally:
            try:
                if exceptional_exit is not None:
                    process.kill()
            finally:
                process.wait()
    wall = time.perf_counter() - started
    result = read_artifact(result_path)
    row = {**run, 'id': identity, 'phase': args.phase, 'ordinal': ordinal,
           'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
           'command': command, 'returncode': process.returncode, 'parent_wall_seconds': wall,
           'peak_memory_bytes': memory.peak, 'memory_observed': memory.available, 'killed': killed,
           'request_sha256': sha(request), 'artifact': str(result_path.relative_to(args.output)),
           'result_sha256': sha(result_path) if result else None,
           'serialization': read_artifact(result_path.with_suffix('.timing.json')),
           'result': result}
    if exceptional_exit is not None:
        row['exception_type'] = type(exceptional_exit).__name__
        row['exception_message'] = str(exceptional_exit)
    append_record(args.output, row)
    print(f"{identity}: {row['failures'] or 'ok'}; {wall:.3f}s; {memory.peak / 1048576:.1f} MiB", flush=True)
    if exceptional_exit is not None:
        raise exceptional_exit


def failures(row):
    problems = []
    if row.get('killed'):
        problems.append(row['killed'])
    if row.get('returncode') != 0:
        problems.append('process_failure')
    if not row.get('memory_observed'):
        problems.append('memory_unobserved')
    if row.get('peak_memory_bytes', 0) > MEMORY_LIMIT:
        problems.append('memory_limit')
    result = row.get('result')
    if result is None:
        problems.append('missing_result')
    else:
        if not result.get('valid') or result.get('required_validation') != 'passed':
            problems.append('invalid')
        if result.get('budget_violation') or result['compute_seconds'] > result['budget_seconds'] + .25:
            problems.append('compute_budget')
        if any(not isinstance(result.get(k), (int, float)) or not math.isfinite(result[k]) or result[k] <= 0
               for k in ('score', 'player_seconds')):
            problems.append('nonpositive_or_nonfinite_metric')
        for category in ('missing_class', 'missing_zone', 'late_timed'):
            texts = result.get('warnings', {}).get(category, [])
            ids = result.get('warning_quest_ids', {}).get(category, [])
            if len(texts) != len(ids) or any(type(quest_id) is not int for quest_id in ids):
                problems.append('warning_identity_missing')
    return sorted(set(problems))


def run_summary(row):
    if row is None:
        return None
    # Full traces/guides stay in canonical runs.jsonl and per-run artifacts. Selection
    # and analysis repeat only references and the metrics needed to assess each pair.
    summary = {key: row[key] for key in (
        'id', 'artifact', 'phase', 'profile', 'seed', 'arm', 'returncode', 'killed',
        'parent_wall_seconds', 'peak_memory_bytes', 'memory_observed', 'failures',
        'request_sha256', 'result_sha256', 'serialization') if key in row}
    result = row.get('result')
    summary['result'] = None if result is None else {key: result[key] for key in (
        'score', 'player_seconds', 'hidden_penalty_seconds', 'compute_seconds', 'budget_seconds',
        'reserve_seconds', 'budget_violation', 'valid', 'required_validation', 'required_quest_count',
        'breakdown', 'warnings', 'warning_quest_ids', 'stages_seconds', 'buy_count', 'copper_end', 'zones') if key in result}
    return summary


def paired(rows, phase, arm):
    panel = [r for r in rows if r['phase'] == phase]
    index = {(r['profile'], r['seed'], r['arm']): r for r in panel}
    output = []
    for profile in PROFILES:
        for seed in ((20261107, 314159) if phase == 'heldout' else (42, 1337)):
            baseline = index.get((profile['id'], seed, 'baseline'))
            candidate = index.get((profile['id'], seed, arm))
            pair = {'profile': profile['id'], 'seed': seed,
                    'baseline': run_summary(baseline), 'candidate': run_summary(candidate)}
            if not baseline or not candidate or failures(baseline) or failures(candidate):
                pair['usable'] = False
                pair['new_failure'] = True
            else:
                b, c = baseline['result'], candidate['result']
                warnings = ('missing_class', 'missing_zone', 'late_timed')
                new_warning = any(set(c.get('warning_quest_ids', {}).get(k, [])) - set(b.get('warning_quest_ids', {}).get(k, [])) for k in warnings)
                required_counts = ('missing_class_quests', 'missing_zone_quests', 'missing_dungeons', 'missing_profession_quests')
                new_missing = any(c['breakdown'].get(k, 0) > b['breakdown'].get(k, 0) for k in required_counts)
                pair.update(usable=True, gain_ratio=c['score'] / b['score'],
                            play_ratio=c['player_seconds'] / b['player_seconds'],
                            score_delta=b['score'] - c['score'], new_failure=bool(new_warning or new_missing))
            output.append(pair)
    return output


def analysis(rows, phase, arm):
    pairs = paired(rows, phase, arm)
    usable = [p for p in pairs if p['usable']]
    metrics = aggregate(usable)
    complete = len(usable) == 16
    metrics.update(arm=arm, phase=phase, complete=complete, usable_pairs=len(usable),
                   practical_win=complete and metrics['practical_win'],
                   any_pair_failure=any(p['new_failure'] for p in pairs))
    metrics['practical_win'] &= not metrics['any_pair_failure']
    return {'metrics': metrics, 'pairs': pairs}


def run_phase(args):
    frozen = args.output / 'protocol.json'
    candidate = None
    if args.phase != 'pilot':
        protocol = read(frozen)
        if (args.budget, args.reserve) != (protocol['budget_seconds'], protocol['reserve_seconds']):
            raise ValueError('budget/reserve differ from frozen protocol')
        if source_identity(args.binary) != protocol['identity']:
            raise ValueError('binary, working tree source, data, requests, or runner changed since freeze')
    if args.phase == 'heldout':
        selection = read(args.output / 'candidate.json')
        if selection['protocol_sha256'] != sha(frozen):
            raise ValueError('candidate belongs to another protocol')
        candidate = selection['arm']
    existing = records(args.output)
    done = {(r['phase'], r['profile'], r['seed'], r['arm']) for r in existing}
    if args.phase == 'tuning' and any(r['phase'] == 'heldout' for r in existing):
        raise ValueError('cannot tune after opening heldout results')
    with experiment_lock():
        for ordinal, run in enumerate(schedule(args.phase, candidate)):
            key = (args.phase, run['profile'], run['seed'], run['arm'])
            if key not in done:
                run_one(args, run, ordinal)
    if args.phase != 'pilot' and source_identity(args.binary) != protocol['identity']:
        raise ValueError('source/data/binary changed while the experiment was running; phase is invalid')


def freeze_protocol(args):
    rows = records(args.output)
    pilot = [r for r in rows if r['phase'] == 'pilot']
    if len(pilot) != 8 or any(failures(r) for r in pilot):
        raise ValueError('need all 8 successful pilot runs before freezing (retain failed pilot in a separate attempt directory)')
    if any((r['result']['budget_seconds'], r['result']['reserve_seconds']) != (args.budget, args.reserve) for r in pilot):
        raise ValueError('freeze must use the budget/reserve actually observed in the pilot')
    if any(r['phase'] != 'pilot' for r in rows):
        raise ValueError('protocol must freeze before tuning')
    write_new(args.output / 'protocol.json', {
        'experiment': 'single_start_search_kernel_not_production_tournament',
        'budget_seconds': args.budget, 'reserve_seconds': args.reserve, 'peak_memory_limit_bytes': MEMORY_LIMIT,
        'compute_tolerance_seconds': .25, 'parent_timeout_extra_seconds': 10,
        'arms': {'baseline': {}, 'chain': {'chain_insert': .05}, 'hub': {'hub_repair': .02}, 'exact': {'eval_window': 0}},
        'identity': source_identity(args.binary), 'pilot_result_sha256': sha(args.output / 'runs.jsonl'),
        'tuning_order': schedule('tuning'), 'heldout_seeds': [20261107, 314159],
        'criterion': '>=1% equal-profile geometric score gain; >=6/8 positive two-seed means; worst pair regression <=2%; player geometric regression <=.25%; no new validity/resource failures',
        'note': args.reason})


def freeze_candidate(args):
    if any(r['phase'] == 'heldout' for r in records(args.output)):
        raise ValueError('cannot select candidate after opening heldout')
    protocol = read(args.output / 'protocol.json')
    if source_identity(args.binary) != protocol['identity']:
        raise ValueError('experiment identity changed before candidate freeze')
    rows = records(args.output)
    analyses = [analysis(rows, 'tuning', arm) for arm in ARMS[1:]]
    tuning = [r for r in rows if r['phase'] == 'tuning']
    if len(tuning) != 64:
        raise ValueError('retain and finish all 64 tuning runs before selecting')
    eligible = [a for a in analyses if a['metrics']['complete'] and not a['metrics']['any_pair_failure']]
    # If all are broken, still retain one candidate for an explicitly negative/inconclusive holdout.
    pool = eligible or [a for a in analyses if a['metrics']['score_ratio'] is not None]
    if not pool:
        raise ValueError('no arm produced any usable pair; repair experiment before heldout')
    chosen = min(pool, key=lambda a: (not a['metrics']['complete'], a['metrics']['score_ratio']))
    write_new(args.output / 'candidate.json', {'arm': chosen['metrics']['arm'],
        'protocol_sha256': sha(args.output / 'protocol.json'), 'tuning_sha256': sha(args.output / 'runs.jsonl'),
        'selection': 'equal-profile geometric-mean final score among feasible arms',
        'feasible_candidate_available': bool(eligible), 'analyses': analyses})
    print(f"Frozen candidate: {chosen['metrics']['arm']} (feasible={bool(eligible)})")


def report(args):
    rows = records(args.output)
    frozen = read(args.output / 'candidate.json') if (args.output / 'candidate.json').exists() else None
    report_data = {'experiment': 'single_start_search_kernel_not_production_tournament',
                   'tuning': [analysis(rows, 'tuning', arm) for arm in ARMS[1:]],
                   'heldout': analysis(rows, 'heldout', frozen['arm']) if frozen else None,
                   'failed_runs': [r['id'] for r in rows if failures(r)],
                   'all_runs': len(rows), 'promotion': 'off by default; production tournament validation still required'}
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%f')
    path = args.output / f'analysis-{stamp}.json'
    write_new(path, report_data)
    with (args.output / f'pairs-{stamp}.csv').open('x', encoding='utf-8', newline='') as stream:
        writer = csv.writer(stream)
        writer.writerow(['phase', 'arm', 'profile', 'seed', 'usable', 'new_failure',
                         'baseline_score', 'candidate_score', 'baseline_player_seconds', 'candidate_player_seconds',
                         'score_delta', 'candidate_baseline_score_ratio', 'candidate_baseline_player_ratio'])
        for section in report_data['tuning'] + ([report_data['heldout']] if report_data['heldout'] else []):
            for pair in section['pairs']:
                b = (pair.get('baseline') or {}).get('result') or {}
                c = (pair.get('candidate') or {}).get('result') or {}
                writer.writerow([section['metrics']['phase'], section['metrics']['arm'], pair['profile'], pair['seed'],
                                 pair['usable'], pair['new_failure'], b.get('score'), c.get('score'),
                                 b.get('player_seconds'), c.get('player_seconds'), pair.get('score_delta'),
                                 pair.get('gain_ratio'), pair.get('play_ratio')])
    with (args.output / f'anytime-{stamp}.csv').open('x', encoding='utf-8', newline='') as stream:
        writer = csv.writer(stream)
        writer.writerow(['phase', 'profile', 'seed', 'arm', 'compute_seconds', 'score', 'player_seconds', 'stage'])
        for row in rows:
            for point in (row.get('result') or {}).get('anytime', []):
                writer.writerow([row['phase'], row['profile'], row['seed'], row['arm'], point['seconds'],
                                 point['score'], point['player_seconds'], point['stage']])
    print(path)
    for section in report_data['tuning'] + ([report_data['heldout']] if report_data['heldout'] else []):
        print(json.dumps(section['metrics'], sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('run', 'freeze-protocol', 'freeze-candidate', 'analyze'))
    parser.add_argument('--phase', choices=('pilot', 'tuning', 'heldout'), default='pilot')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/examples' / ('quality_compare.exe' if os.name == 'nt' else 'quality_compare'))
    parser.add_argument('--budget', type=float, default=20)
    parser.add_argument('--reserve', type=float, default=3)
    parser.add_argument('--reason', default='Initial 20-second computation allowance and 3-second uniform reserve; pilot passed.')
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    if not math.isfinite(args.budget) or not math.isfinite(args.reserve) or not 0 <= args.reserve < args.budget:
        parser.error('require finite budget > reserve >= 0')
    {'run': run_phase, 'freeze-protocol': freeze_protocol,
     'freeze-candidate': freeze_candidate, 'analyze': report}[args.command](args)


if __name__ == '__main__':
    main()
