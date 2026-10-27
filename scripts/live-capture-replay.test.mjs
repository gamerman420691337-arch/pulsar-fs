// Authored CPU owner-death recovery through the shipped CLI/local IPC.
import test from 'node:test';
import {captureReplay} from './live-capture-replay-fixture.mjs';
const worker=process.env.PULSAR_BENCHMARK_WORKER;
const runtime=process.env.PULSAR_BENCHMARK_ORT;
test('CLI replays exact Start and both Continue receipts after owned Engine SIGKILL without rearming', {skip:!worker||!runtime||process.platform!=='linux',timeout:90000}, t=>captureReplay(t,{worker,runtime}));
