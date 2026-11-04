// Actual CUDA worker, authenticated CLI/local IPC and confirmed owner SIGKILL.
import test from 'node:test';
import {captureReplay} from './live-capture-replay-fixture.mjs';
const worker=process.env.PULSAR_CUDA_CLI_WORKER;
const runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const ownerPolicy=process.env.PULSAR_CUDA_CLI_POLICY;
const declaredProfile=process.env.PULSAR_CUDA_CLI_PROFILE;
const authoredModel=process.env.PULSAR_CUDA_CLI_MODEL;
test('CUDA CLI survives owner SIGKILL, replays inactive receipts and completes fresh Finish', {skip:!worker||!runtime||!ownerPolicy||!declaredProfile||!authoredModel||process.platform!=='linux',timeout:240000},t=>captureReplay(t,{worker,runtime,ownerPolicy,declaredProfile,authoredModel,provider:'cuda'}));
