// Explicit opt-in prepared Start/Continue/ContinueAt receipt and owner-death proofs.
import test from 'node:test';
import {captureReplay} from './live-capture-replay-fixture.mjs';
const cpuWorker=process.env.PULSAR_BENCHMARK_WORKER,cpuRuntime=process.env.PULSAR_BENCHMARK_ORT;
test('native CPU predecessor accepts fresh work during preparation and successor uses its own pair identity',
  {skip:!cpuWorker||!cpuRuntime||process.platform!=='linux',timeout:90000},
  t=>captureReplay(t,{worker:cpuWorker,runtime:cpuRuntime,scenario:'cpu_overlap'}));
test('prepared CPU continuation and inherited derived mode replay after owner death without rearming',
  {skip:!cpuWorker||!cpuRuntime||process.platform!=='linux',timeout:90000},
  t=>captureReplay(t,{worker:cpuWorker,runtime:cpuRuntime,prepared:true}));
const worker=process.env.PULSAR_CUDA_CLI_WORKER,runtime=process.env.PULSAR_CUDA_CLI_RUNTIME;
const ownerPolicy=process.env.PULSAR_CUDA_CLI_POLICY,declaredProfile=process.env.PULSAR_CUDA_CLI_PROFILE;
const authoredModel=process.env.PULSAR_CUDA_CLI_MODEL;
test('prepared CUDA receipts and inherited mode remain historical after owner death with exact private overlap capacity',
  {skip:!worker||!runtime||!ownerPolicy||!declaredProfile||!authoredModel||process.platform!=='linux',timeout:240000},
  t=>captureReplay(t,{worker,runtime,ownerPolicy,declaredProfile,authoredModel,provider:'cuda',prepared:true,overlapMaximumReservedBytes:256*2**20}));
