import test from 'node:test';
import {captureReplay} from './live-capture-replay-fixture.mjs';
const worker=process.env.PULSAR_CUDA_CLI_WORKER;
const runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const ownerPolicy=process.env.PULSAR_CUDA_CLI_POLICY;
const declaredProfile=process.env.PULSAR_CUDA_CLI_PROFILE;
const authoredModel=process.env.PULSAR_CUDA_CLI_MODEL;
test('CUDA CLI Stop cancels frozen-worker Finish, replays terminal receipts and completes a fresh capture',
  {skip:!worker||!runtime||!ownerPolicy||!declaredProfile||!authoredModel||process.platform!=='linux',timeout:120000},
  t=>captureReplay(t,{worker,runtime,ownerPolicy,declaredProfile,authoredModel,provider:'cuda',scenario:'blocked_finish'}));
