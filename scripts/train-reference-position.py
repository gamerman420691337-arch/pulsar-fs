#!/usr/bin/env python3
"""Private training-only temporal script-position candidate; no qualification."""
import argparse
import bisect
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import stat
import subprocess
import time
from fractions import Fraction

JSON_LIMIT = 8 * 1024 * 1024
WIDTH = HEIGHT = 64
MAX_FRAMES = 4096


def require(condition, message):
    if not condition:
        raise ValueError(message)


def hash_file(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    return digest.hexdigest()


def read_json(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        metadata = os.fstat(fd)
        require(stat.S_ISREG(metadata.st_mode) and metadata.st_size <= JSON_LIMIT,
                'JSON type or byte admission')
        with os.fdopen(fd, 'rb', closefd=False) as stream:
            data = stream.read(JSON_LIMIT + 1)
        require(len(data) <= JSON_LIMIT, 'JSON grew beyond admission')
        return json.loads(data), data
    finally:
        os.close(fd)


def reference_actions(value):
    require(isinstance(value, dict) and isinstance(value.get('actions'), list), 'reference actions missing')
    actions = value['actions']
    require(2 <= len(actions) <= 250000, 'reference action count admission')
    result = []
    for action in actions:
        require(isinstance(action, dict), 'reference action object required')
        t, p = action.get('at'), action.get('pos')
        require(type(t) is int and 0 <= t <= 86400000 and type(p) is int and 0 <= p <= 100,
                'reference requires bounded integer milliseconds and positions')
        require(not result or t > result[-1][0], 'reference timestamps must strictly increase')
        result.append((t, p))
    return result


def label_at(actions, times, elapsed):
    """Interpolate supplied script at exact script time; no extrapolation."""
    milliseconds = elapsed * 1000
    if milliseconds < times[0] or milliseconds > times[-1]:
        return None
    i = bisect.bisect_right(times, milliseconds) - 1
    if i == len(actions) - 1:
        return actions[i][1] / 100
    left, right = actions[i], actions[i + 1]
    p = Fraction(left[1]) + Fraction(right[1] - left[1]) * (milliseconds - left[0]) / (right[0] - left[0])
    return float(p / 100)


def bounded_process(command, stdout_limit, stderr_limit, timeout, pass_fds=()):
    """Drain both pipes with bounded storage and a deadline; kill/reap on error."""
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             pass_fds=pass_fds)
    deadline = time.monotonic() + timeout
    outputs = [bytearray(), bytearray()]
    selector = selectors.DefaultSelector()
    try:
        selector.register(child.stdout, selectors.EVENT_READ, 0)
        selector.register(child.stderr, selectors.EVENT_READ, 1)
        while selector.get_map():
            require(time.monotonic() < deadline, 'decoder/tool deadline exceeded')
            for key, _ in selector.select(min(0.1, max(0, deadline - time.monotonic()))):
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                outputs[key.data].extend(chunk)
                require(len(outputs[key.data]) <= (stdout_limit, stderr_limit)[key.data], 'decoder/tool byte admission exceeded')
        require(child.wait(timeout=max(0.01, deadline-time.monotonic())) == 0, 'decoder/tool failed')
        return bytes(outputs[0]), bytes(outputs[1])
    finally:
        if child.poll() is None:
            child.kill()
        child.wait()
        selector.close()
        child.stdout.close()
        child.stderr.close()


def snapshot(source, destination, remaining, per_file):
    fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        before = os.fstat(fd)
        require(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= min(remaining, per_file), 'source type/byte admission')
        digest = hashlib.sha256()
        count = 0
        out = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        try:
            with os.fdopen(fd, 'rb', closefd=False) as inp, os.fdopen(out, 'wb', closefd=False) as target:
                for chunk in iter(lambda: inp.read(1024 * 1024), b''):
                    count += len(chunk)
                    require(count <= min(remaining, per_file), 'growing source byte admission')
                    target.write(chunk)
                    digest.update(chunk)
                target.flush()
                os.fsync(out)
            after = os.fstat(fd)
            require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) ==
                    (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) and count == before.st_size,
                    'source changed during snapshot')
            os.fchmod(out, 0o400)
        finally:
            os.close(out)
        return {'sha256': digest.hexdigest(), 'byte_len': count, 'path': str(destination.name)}
    finally:
        os.close(fd)


def write_json(path, value):
    with Path(path).open('x', encoding='utf8') as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write('\n')


def reference_offset(entry):
    """Declared script milliseconds at source elapsed zero; no inferred alignment."""
    value = entry.get('reference_offset_ms', 0)
    require(type(value) is int and abs(value) <= 86400000, 'reference clock offset must be bounded integer milliseconds')
    return Fraction(value, 1000)


def validate_reference_offsets(manifest, intake):
    """Require staged clock declarations to match their exact bound manifest."""
    offsets = {entry['id']: reference_offset(entry) for entry in manifest['entries']}
    entries = intake['entries']
    require(len(entries) == len(offsets) and {entry['id'] for entry in entries} == set(offsets),
            'reference offset entry identities differ')
    require(all(reference_offset(entry) == offsets[entry['id']] for entry in entries),
            'reference offset differs from bound manifest')


def validate_manifest(value):
    require(isinstance(value, dict) and set(value) == {'schema_version', 'split', 'authorization', 'input_root', 'entries'}, 'training manifest fields')
    require(type(value['schema_version']) is int and value['schema_version'] in (1,2) and value['split'] == 'training', 'only training split admitted')
    require(isinstance(value['authorization'], str) and 1 <= len(value['authorization']) <= 4096, 'training authorization record required')
    require(isinstance(value['input_root'], str), 'training root required')
    root = Path(value['input_root']).resolve(strict=True)
    require(root.is_dir() and root.name == 'Live-Reference', 'only explicitly supplied Live-Reference root admitted')
    entries = value['entries']
    require(isinstance(entries, list) and 1 <= len(entries) <= 64, 'training entries admission')
    ids = set()
    for entry in entries:
        require(isinstance(entry, dict) and set(entry) == ({'id', 'video', 'reference', 'group', 'permitted_uses'} | ({'reference_offset_ms'} if value['schema_version'] == 2 else set())), 'training entry fields')
        require(isinstance(entry['id'], str) and re.fullmatch(r'[a-z0-9_-]{1,64}', entry['id']) and entry['id'] not in ids, 'training ID admission')
        ids.add(entry['id'])
        require(isinstance(entry['group'], str) and re.fullmatch(r'[a-z0-9_-]{1,64}',entry['group']), 'source group required')
        reference_offset(entry)
        require(entry['permitted_uses'] == ['training'], 'training-only rights required; no redistribution inferred')
        for field in ['video', 'reference']:
            relative = entry[field]
            require(isinstance(relative, str) and len(relative) <= 4096 and not Path(relative).is_absolute() and '..' not in Path(relative).parts,
                    'training input must be a contained relative path')
            candidate = root / relative
            require(not candidate.is_symlink() and candidate.resolve(strict=True).is_relative_to(root), 'training input escapes root or is a symlink')
    return root


def decode_clip(video, start, duration, stride, ffmpeg, timebase, timeout,
                spatial_filter='scale=64:64:flags=neighbor'):
    # Input duration bounds decoding; trim removes output beyond the exact
    # window before showinfo. No fps filter, duplication or zero-origin shift.
    filter_value = f'trim=start={start}:end={start+duration},select=not(mod(n\\,{stride})),{spatial_filter},showinfo'
    command = [ffmpeg, '-nostdin', '-copyts', '-ss', str(start), '-t', str(duration), '-threads', '2', '-protocol_whitelist', 'file,pipe', '-noautorotate', '-i', str(video),
               '-map', '0:v:0', '-an', '-sn', '-dn', '-vf', filter_value, '-fps_mode', 'passthrough', '-pix_fmt', 'rgb24', '-threads', '1', '-f', 'rawvideo', 'pipe:1']
    rgb, log = bounded_process(command, MAX_FRAMES*WIDTH*HEIGHT*3, 2*1024*1024, timeout)
    text = log.decode('utf8', errors='strict')
    bases = re.findall(r'config in time_base:\s*(\d+)/(\d+)', text)
    require(bases and all(Fraction(int(n),int(d)) == timebase for n,d in bases), 'decoded filter clock differs from probed source clock')
    pts = [int(v) for v in re.findall(r'\bn:\s*\d+\s+pts:\s*(-?\d+)\s+pts_time:', text)]
    require(len(rgb) % (WIDTH*HEIGHT*3) == 0 and len(pts) == len(rgb)//(WIDTH*HEIGHT*3) and 2 <= len(pts) <= MAX_FRAMES, 'RGB/PTS frame correspondence admission')
    require(all(a < b for a,b in zip(pts,pts[1:])), 'decoded PTS must strictly increase')
    exact_start = Fraction(str(start))
    require(all(exact_start <= p*timebase < exact_start+duration for p in pts), 'decoded frame outside selected window')
    return rgb, pts, command


def stage(manifest_path, output, windows, duration, stride, ffmpeg, ffprobe,
          view_annotations=None, view_planner=None):
    import numpy as np
    manifest, manifest_bytes = read_json(manifest_path)
    root = validate_manifest(manifest)
    require(output.is_dir(), 'owned training run directory absent')
    require((view_annotations is None) == (view_planner is None), 'input views require a native planner')
    annotations = None
    if view_annotations is not None:
        annotations, annotation_bytes = read_json(view_annotations)
        require(set(annotations) == {'schema_version','entries'} and
                type(annotations['schema_version']) is int and annotations['schema_version'] == 1 and
                isinstance(annotations['entries'],list) and 1 <= len(annotations['entries']) <= 64,
                'input view annotation fields')
        by_id = {}
        for declaration in annotations['entries']:
            require(isinstance(declaration,dict) and set(declaration) == {'id','source_sha256','declaration'} and
                    isinstance(declaration['id'],str) and declaration['id'] not in by_id,
                    'input view entry fields/identity')
            by_id[declaration['id']] = declaration
        require(set(by_id) == {entry['id'] for entry in manifest['entries']}, 'input views must cover exact manifest entries')
        view_planner = Path(view_planner).resolve(strict=True)
        require(view_planner.is_file(), 'native view planner absent')
        planner_hash = hash_file(view_planner)
        with (output/'input-views.json').open('xb') as stream:
            os.chmod(output/'input-views.json',0o600)
            stream.write(annotation_bytes)
    with (output/'manifest.json').open('xb') as stream:
        os.chmod(output/'manifest.json', 0o600)
        stream.write(manifest_bytes)
    records, tensors, labels, times, groups = [], [], [], [], []
    remaining = 32*1024**3
    seen_hashes = set()
    for entry in manifest['entries']:
        source = snapshot(root/entry['video'], output/(entry['id']+'.media'), remaining, 16*1024**3)
        remaining -= source['byte_len']
        require(source['sha256'] not in seen_hashes, 'duplicate source bytes in training corpus')
        seen_hashes.add(source['sha256'])
        reference = snapshot(root/entry['reference'], output/(entry['id']+'.funscript'), remaining, JSON_LIMIT)
        remaining -= reference['byte_len']
        actions = reference_actions(read_json(output/reference['path'])[0])
        action_times = [a[0] for a in actions]
        video = output/source['path']
        probe_bytes, _ = bounded_process([ffprobe,'-v','error','-protocol_whitelist','file,pipe','-select_streams','v:0','-read_intervals','%+#1','-show_entries','stream=time_base,width,height:frame=best_effort_timestamp','-of','json',str(video)], JSON_LIMIT, 65536, 30)
        probe = json.loads(probe_bytes)
        require(len(probe.get('streams',[])) == 1 and len(probe.get('frames',[])) == 1, 'source stream/origin admission')
        stream = probe['streams'][0]
        require(0 < stream['width'] <= 16384 and 0 < stream['height'] <= 16384, 'source dimension admission')
        base = Fraction(stream['time_base'])
        require(base > 0, 'invalid source timebase')
        origin = int(probe['frames'][0]['best_effort_timestamp'])*base
        input_view = None
        if annotations is not None:
            annotation = by_id[entry['id']]
            require(annotation['source_sha256'] == source['sha256'] and
                    annotation['declaration']['source_dimensions'] == [stream['width'],stream['height']],
                    'input view source identity/dimensions differ')
            declaration = json.dumps(annotation['declaration'],separators=(',',':'),allow_nan=False)
            require(len(declaration.encode('utf8')) <= 4096, 'input view declaration byte admission')
            plan_bytes, _ = bounded_process([str(view_planner),'training-input-plan',declaration],65536,65536,30)
            input_view = json.loads(plan_bytes)
            require(input_view['schema_version'] == 1 and input_view['recipe'] == 'annotated-temporal-view-v1' and
                    input_view['declaration'] == annotation['declaration'] and input_view['output_dimensions'] == [64,64] and
                    isinstance(input_view['spatial_filter'],str) and 1 <= len(input_view['spatial_filter']) <= 4096 and
                    hash_file(view_planner) == planner_hash, 'native input view plan identity differs')
        clips = []
        for window in range(windows):
            action = actions[(window+1)*len(actions)//(windows+1)]
            start = max(origin, origin+Fraction(action[0],1000)-reference_offset(entry)-Fraction(duration,2))
            # Decimal seek/trim parameters are exact milliseconds. Keep the
            # rational origin separate for label interpolation and receipt.
            start = Fraction(math.floor(start*1000),1000)
            rgb, pts, command = decode_clip(video,float(start),duration,stride,ffmpeg,base,120,
                input_view['spatial_filter'] if input_view else 'scale=64:64:flags=neighbor')
            frames = np.frombuffer(rgb,dtype=np.uint8).reshape((-1,HEIGHT,WIDTH,3))
            clip_tensor, clip_labels, clip_times = [], [], []
            for i in range(1,len(pts)):
                elapsed = pts[i]*base-origin
                label = label_at(actions,action_times,elapsed+reference_offset(entry))
                if label is None or (pts[i]-pts[i-1])*base > Fraction(1,2):
                    continue
                clip_tensor.append(np.concatenate([frames[i-1],frames[i]],axis=2).transpose(2,0,1))
                clip_labels.append(label)
                clip_times.append([pts[i-1],pts[i]])
            require(len(clip_labels)>0,'selected clip has no supported temporal labels')
            require(len(labels)+len(clip_labels)<=65536 and (len(labels)+len(clip_labels))*6*64*64<=1024**3, 'aggregate training tensor admission')
            tensors.append(np.stack(clip_tensor));labels.extend(clip_labels);times.extend(clip_times);groups.extend([entry['id']]*len(clip_labels))
            clips.append({'window':window,'start':str(start),'duration_seconds':duration,'decoded_pts':pts,'training_samples':len(clip_labels),'omitted_pairs':len(pts)-1-len(clip_labels),'raw_rgb_sha256':hashlib.sha256(rgb).hexdigest(),'command':command})
        require(hash_file(video)==source['sha256'] and hash_file(output/reference['path'])==reference['sha256'], 'snapshot changed during decoding')
        records.append({'id':entry['id'],'group':entry['group'],'reference_offset_ms':int(reference_offset(entry)*1000),'source':source,'reference':reference,'source_timebase':str(base),'source_origin':str(origin),'source_dimensions':[stream['width'],stream['height']],'clips':clips})
        if input_view is not None:
            records[-1]['input_view_plan'] = input_view
        print(json.dumps({'phase':'staged','id':entry['id'],'samples':sum(c['training_samples'] for c in clips)}),flush=True)
    x=np.concatenate(tensors);y=np.asarray(labels,dtype=np.float32).reshape(-1,1)
    require(len(x)<=65536 and x.nbytes+y.nbytes<=1024**3,'aggregate training tensor admission')
    np.savez(output/'dataset.npz',frames=x,labels=y,source_pts=np.asarray(times,dtype=np.int64),groups=np.asarray(groups))
    os.chmod(output/'dataset.npz',0o400)
    record={'schema_version':1,'split':'training','manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(),'dataset_sha256':hash_file(output/'dataset.npz'),'samples':len(x),'preprocessing':{'width':64,'height':64,'channels':6,'layout':'nchw','order':'previous RGB then current RGB','resize':'nearest full packed source; no projection or target selection','orientation':'encoded source; display rotation disabled','scale':1/255},'label':'linear supplied script position at exact elapsed current source PTS','alignment':'script time equals exact source elapsed time plus declared reference_offset_ms (legacy zero); human alignment unverified','entries':records,'rights':'training only; no redistribution authorization','qualification':'unqualified'}
    if annotations is not None:
        require(read_json(view_annotations)[1] == annotation_bytes and hash_file(view_planner) == planner_hash,
                'input views/planner changed during staging')
        record['schema_version'] = 2
        record['input_views_sha256'] = hashlib.sha256(annotation_bytes).hexdigest()
        record['view_planner'] = {'sha256':planner_hash,'byte_len':view_planner.stat().st_size}
        record['preprocessing']['recipe'] = 'annotated-temporal-view-v1'
        record['preprocessing']['resize'] = 'declared native eye/projection view, integer target region, nearest 64x64'
    write_json(output/'intake.json',record)
    return x,y,record


def train(x,y,output,intake,epochs,seed,heldout=None):
    require(intake['schema_version'] == 1 and 'input_views_sha256' not in intake and
            all('input_view_plan' not in entry for entry in intake.get('entries',[])),
            'annotated views require a separately bound model contract; legacy export refused')
    return _train(x,y,output,intake,epochs,seed,heldout)


VIEW_MODEL_CONTRACT = {
    'schema_version':1, 'recipe':'annotated-temporal-view-v1',
    'input_name':'target_frame_pair', 'input_shape':[1,6,64,64],
    'output_name':'script_position', 'output_shape':[1,1],
    'dtype':'float32', 'layout':'nchw', 'normalization':'byte / 255', 'output_range':[0,1],
    'channel_order':'previous RGB then current RGB',
    'orientation':'encoded source; display rotation disabled',
    'view_policy':'declared native eye/projection, static integer target region, nearest 64x64',
}


def train_annotated(x,y,output,intake,epochs,seed,heldout):
    require(intake['schema_version'] == 2 and intake['preprocessing']['recipe'] == VIEW_MODEL_CONTRACT['recipe'] and
            heldout is not None and len(heldout[0]) > 0, 'annotated model requires bound recipe and separate development data')
    return _train(x,y,output,intake,epochs,seed,heldout,annotated=True)


def position_network(torch):
    """Construct the shared small CPU candidate; construction is not qualification."""
    return torch.nn.Sequential(torch.nn.Conv2d(6,16,5,stride=2,padding=2),torch.nn.ReLU(),torch.nn.Conv2d(16,32,3,stride=2,padding=1),torch.nn.ReLU(),torch.nn.Conv2d(32,32,3,stride=2,padding=1),torch.nn.ReLU(),torch.nn.AvgPool2d(2),torch.nn.Flatten(),torch.nn.Linear(512,64),torch.nn.ReLU(),torch.nn.Linear(64,1),torch.nn.Sigmoid())


def _train(x,y,output,intake,epochs,seed,heldout=None,annotated=False):
    input_name = VIEW_MODEL_CONTRACT['input_name'] if annotated else 'frame_pair'
    import numpy as np
    import torch
    import onnx  # Check exporter dependencies before fitting, not after epochs.
    import onnxruntime as ort
    torch.set_num_threads(4);torch.manual_seed(seed);torch.use_deterministic_algorithms(True)
    network=position_network(torch)
    inputs=torch.from_numpy(x);targets=torch.from_numpy(y);optimizer=torch.optim.Adam(network.parameters(),lr=0.001)
    generator=torch.Generator().manual_seed(seed);losses=[]
    for epoch in range(epochs):
        network.train();total=0.0
        for indices in torch.randperm(len(inputs),generator=generator).split(32):
            prediction=network(inputs[indices].float()/255);loss=torch.nn.functional.mse_loss(prediction,targets[indices]);require(bool(torch.isfinite(loss)), 'nonfinite training loss')
            optimizer.zero_grad(set_to_none=True);loss.backward();torch.nn.utils.clip_grad_norm_(network.parameters(),1.0);optimizer.step();total+=float(loss.detach())*len(indices)
        losses.append(total/len(inputs));print(json.dumps({'phase':'epoch','epoch':epoch+1,'mse':losses[-1]}),flush=True)
    network.eval()
    torch.save(network.state_dict(),output/'weights.pt');os.chmod(output/'weights.pt',0o400)
    example=inputs[:1].float()/255
    # Fixed-shape legacy exporter is explicitly pinned here; runtime parity is
    # measured below. This is not a claim about all exporter implementations.
    torch.onnx.export(network,(example,),str(output/'model.onnx'),input_names=[input_name],output_names=['script_position'],opset_version=17,dynamo=False,external_data=False)
    if annotated:
        exported = onnx.load(str(output/'model.onnx'),load_external_data=False)
        onnx.helper.set_model_props(exported,{
            'pulsar.temporal_recipe':json.dumps(VIEW_MODEL_CONTRACT,sort_keys=True,separators=(',',':')),
            'pulsar.training_intake_sha256':hash_file(output/'intake.json'),
        })
        onnx.checker.check_model(exported)
        onnx.save_model(exported,str(output/'model.onnx'))
    os.chmod(output/'model.onnx',0o400)
    options=ort.SessionOptions();options.intra_op_num_threads=4;options.inter_op_num_threads=1
    session=ort.InferenceSession(str(output/'model.onnx'),options,providers=['CPUExecutionProvider'])
    require(len(session.get_inputs())==1 and session.get_inputs()[0].name==input_name and session.get_inputs()[0].type=='tensor(float)' and session.get_inputs()[0].shape==[1,6,64,64] and len(session.get_outputs())==1 and session.get_outputs()[0].name=='script_position' and session.get_outputs()[0].type=='tensor(float)' and session.get_outputs()[0].shape==[1,1], 'exported tensor contract differs')
    errors=[];predictions=[]
    with torch.inference_mode():
        for i in range(len(inputs)):
            frame=inputs[i:i+1].float()/255;p=float(network(frame)[0,0]);predictions.append(p)
            if i < 16:
                q=session.run(None,{input_name:frame.numpy()})[0];require(q.shape==(1,1) and np.isfinite(q).all() and 0<=float(q[0,0])<=1,'invalid ONNX position');errors.append(abs(p-float(q[0,0])))
    require(max(errors)<=1e-5,'PyTorch/ONNX parity exceeded tolerance')
    predicted=np.asarray(predictions,dtype=np.float32);actual=y[:,0];mean=float(actual.mean())
    receipt={'schema_version':1,'task':'temporal-reference-script-position-v1','training_only':True,'qualification':'unqualified','seed':seed,'epochs':epochs,'device':'cpu','threads':4,'torch_version':torch.__version__,'onnxruntime_version':ort.__version__,'intake_sha256':hash_file(output/'intake.json'),'dataset_sha256':intake['dataset_sha256'],'training_samples':len(x),'epoch_mse':losses,'training_mae':float(np.abs(predicted-actual).mean()),'training_mean_baseline_mae':float(np.abs(mean-actual).mean()),'parity_samples':len(errors),'maximum_onnx_absolute_error':max(errors),'model':{'path':'model.onnx','sha256':hash_file(output/'model.onnx'),'byte_len':(output/'model.onnx').stat().st_size},'weights_sha256':hash_file(output/'weights.pt'),'abi':{'input_name':'frame_pair','shape':[1,6,64,64],'dtype':'float32','normalization':'byte / 255','channel_order':'previous RGB then current RGB','output_name':'script_position','output_shape':[1,1],'output_range':[0,1]},'sterile_evaluation_performed':False,'production_worker_integration':False,'heldout_evaluation_performed':False,'physical_motion_qualification':False,'limitations':['Labels are supplied reference script positions, not independently measured physical motion.','No target boxes, anatomical identity, depth or confidence calibration.','Training loss/MAE are resubstitution statistics, not generalization evidence.','Source/script alignment and packed-source interpretation remain unverified.','Candidate is private and unqualified; no distribution or runtime activation authorized by this receipt.']}
    if annotated:
        receipt['schema_version']=2
        receipt['task']='annotated-temporal-reference-script-position-v1'
        receipt['abi']['input_name']=input_name
        receipt['input_recipe']=VIEW_MODEL_CONTRACT
        receipt['limitations']=[
            'Declared view/region does not verify target identity or anatomical/physical motion.',
            'Training MAE is resubstitution, not generalization evidence.',
            'Visual/script alignment, resolution/cadence adequacy and independent quality remain unverified.',
            'Separate production profile/runtime binding and qualification remain required.',
        ]
        metadata=session.get_modelmeta().custom_metadata_map
        require(json.loads(metadata.get('pulsar.temporal_recipe','null'))==VIEW_MODEL_CONTRACT and
                metadata.get('pulsar.training_intake_sha256')==receipt['intake_sha256'],
                'exported annotated model recipe/lineage differs')
    if heldout is not None:
        receipt['scope']='optimizer and export receipt; subsequent holdout measurement is recorded separately'
        receipt['heldout_evaluation_planned']=True
    write_json(output/'training.json',receipt)
    if heldout is not None:
        heldout_x,heldout_y,heldout_groups=heldout
        predictions=[];heldout_errors=[]
        with torch.inference_mode():
            for sample in heldout_x:
                frame=sample[np.newaxis].astype(np.float32)/255
                value=session.run(None,{input_name:frame})[0]
                require(value.shape==(1,1) and np.isfinite(value).all() and 0<=float(value[0,0])<=1,'invalid heldout ONNX position')
                predictions.append(float(value[0,0]))
                heldout_errors.append(abs(predictions[-1]-float(network(torch.from_numpy(frame))[0,0])))
        require(max(heldout_errors)<=1e-5,'heldout export parity exceeded tolerance')
        actual=heldout_y[:,0];predicted=np.asarray(predictions)
        median=float(np.median(y[:,0]))
        def metrics(mask):
            return {'samples':int(mask.sum()),'onnx_mae':float(np.abs(predicted[mask]-actual[mask]).mean()),'training_mean_baseline_mae':float(np.abs(mean-actual[mask]).mean()),'training_median_baseline_mae':float(np.abs(median-actual[mask]).mean())}
        result={'schema_version':1,'state':'measured','qualification':'unqualified','scope':'live-reference development holdout; not sterile or independent release evaluation','model_sha256':receipt['model']['sha256'],'training_receipt_sha256':hash_file(output/'training.json'),'split_receipt_sha256':hash_file(output/'split.json'),'optimizer_samples':len(x),'heldout_samples':len(heldout_x),'constant_training_mean':mean,'constant_training_median':median,'aggregate':metrics(np.ones(len(actual),dtype=bool)),'by_declared_group':{str(group):metrics(heldout_groups==group) for group in sorted(set(heldout_groups))},'parity_samples':len(heldout_errors),'maximum_onnx_absolute_error':max(heldout_errors),'sterile_evaluation_performed':False,'selection':'fixed epochs; no heldout checkpoint, threshold or hyperparameter selection','limitations':['Group declarations are not independently verified source-family identity.','Supplied script alignment and physical labels remain unverified.','Development holdout metrics do not qualify stroke generation or physical playback.']}
        write_json(output/'holdout.json',result)
        print(json.dumps({'phase':'heldout','samples':len(heldout_x),**result['aggregate']}),flush=True)
    print(json.dumps({'phase':'completed','samples':len(x),'training_mae':receipt['training_mae'],'mean_baseline_mae':receipt['training_mean_baseline_mae'],'maximum_onnx_error':max(errors),'model_sha256':receipt['model']['sha256']}),flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--manifest',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--epochs',type=int,default=10);parser.add_argument('--windows',type=int,default=4);parser.add_argument('--duration',type=int,default=12);parser.add_argument('--stride',type=int,default=4);parser.add_argument('--seed',type=int,default=20261004);args=parser.parse_args()
    require(1<=args.epochs<=100 and 1<=args.windows<=16 and 1<=args.duration<=30 and 1<=args.stride<=60,'training parameter admission')
    # Exclusive creation precedes the failure handler: an existing run or
    # unrelated directory must never receive this invocation's failure file.
    validate_manifest(read_json(args.manifest)[0])
    args.output.mkdir(mode=0o700, parents=False)
    try:
        x,y,intake=stage(args.manifest,args.output,args.windows,args.duration,args.stride,'/usr/bin/ffmpeg','/usr/bin/ffprobe');train(x,y,args.output,intake,args.epochs,args.seed)
    except BaseException as error:
        if args.output.is_dir() and not (args.output/'failure.json').exists():
            write_json(args.output/'failure.json',{'schema_version':1,'state':'failed','error_type':type(error).__name__,'message':str(error)[:2048],'qualification':'unqualified'})
        raise


if __name__=='__main__':
    main()
