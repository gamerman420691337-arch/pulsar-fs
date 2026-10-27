"""Private development-worker intake from frozen, hash-bound live training clips.

No optimizer, sterile intake, media redistribution or activation. This adapter
refuses omitted/discontinuous staged pairs rather than reconstructing pixels.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
from fractions import Fraction

spec = importlib.util.spec_from_file_location('reference_holdout', Path(__file__).with_name('train-reference-holdout.py'))
holdout = importlib.util.module_from_spec(spec)
spec.loader.exec_module(holdout)
training = holdout.training
require = training.require


def reconstruct(frames, pairs, clip, timebase):
    """Recover exactly the originally staged RGB/PTS sequence, without filling gaps."""
    import numpy as np
    count = len(frames)
    pts = clip['decoded_pts']
    require(1 <= count <= 256 and len(pts) == count + 1 and clip['omitted_pairs'] == 0,
            'only complete bounded staged clips admitted')
    require(pairs.tolist() == [[a,b] for a,b in zip(pts,pts[1:])], 'staged pair clocks differ')
    require(all(type(p) is int and abs(p) <= 2**53 for p in pts), 'exact encoder integer clock admission')
    require(all(0 < (b-a)*timebase <= Fraction(1,2) for a,b in zip(pts,pts[1:])), 'staged clock gaps differ')
    require(all(np.array_equal(frames[i-1,3:],frames[i,:3]) for i in range(1,count)), 'adjacent captured pixels differ')
    rgb = b''.join(frame.transpose(1,2,0).tobytes() for frame in [frames[0,:3],*frames[:,3:]])
    require(hashlib.sha256(rgb).hexdigest() == clip['raw_rgb_sha256'], 'reconstructed RGB differs from original decode')
    return rgb, pts


def reference_for_clip(actions, first, last, origin, subject):
    """Retain only complete supplied edges; subtract exact clip origin once."""
    def ns(t):
        value = (t-first)*1_000_000_000
        return str(value.numerator//value.denominator)
    end = ns(last)
    require(0 < int(end) <= 2**63-1, 'evaluation clock bound')
    strokes = []
    for i,(a,b) in enumerate(zip(actions,actions[1:])):
        at = origin + Fraction(a[0],1000)
        bt = origin + Fraction(b[0],1000)
        if at < first or bt > last or a[1] == b[1]:
            continue
        # Supplied integer percent positions retain exact ppm scaling.
        require(int(ns(at)) < int(ns(bt)), 'reference quantization loses ordering')
        strokes.append({'id':f'supplied-edge-{i}','target_id':'declared-script','axis':'stroke',
                        'start_ns':ns(at),'end_ns':ns(bt),'start_position_ppm':a[1]*10000,
                        'end_position_ppm':b[1]*10000})
    require(len(strokes) <= 5000, 'reference edge bound')
    return {'schema_version':1,'subject_id':subject,'window':{'start_ns':'0','end_ns':end},
            'strokes':strokes,'material_sections':[]}


def identity(path):
    digest = training.hash_file(path)
    return {'artifact_id':digest,'sha256':digest,'byte_len':path.stat().st_size}


def encode_clip(root, rgb, pts, base, ffmpeg, ffprobe):
    raw = root/'frames.rgb'
    raw.write_bytes(rgb)
    os.chmod(raw,0o400)
    encoded = root/'source.nut'
    expression = '+'.join(f'eq(N,{i})*{p}' for i,p in enumerate(pts))
    command = [str(ffmpeg),'-v','error','-nostdin','-f','rawvideo','-pixel_format','rgb24',
               '-video_size','64x64','-framerate','25','-i',str(raw),'-vf',
               f"settb=expr={base.numerator}/{base.denominator},setpts='{expression}'",
               '-fps_mode','passthrough','-enc_time_base',str(base),'-c:v','rawvideo','-pix_fmt','rgb24',
               '-threads','1','-f','nut',str(encoded)]
    training.bounded_process(command,65536,65536,30)
    probe_command = [str(ffprobe),'-v','error','-protocol_whitelist','file,pipe','-select_streams','v:0',
                     '-show_entries','stream=time_base,width,height:frame=best_effort_timestamp','-show_frames',
                     '-of','json',str(encoded)]
    metadata,_ = training.bounded_process(probe_command,2*1024*1024,65536,30)
    probe = json.loads(metadata)
    require(len(probe['streams']) == 1 and probe['streams'][0]['width'] == 64 and probe['streams'][0]['height'] == 64,
            'derived stream dimensions differ')
    actual_base = Fraction(probe['streams'][0]['time_base'])
    actual = [int(frame['best_effort_timestamp'])*actual_base for frame in probe['frames']]
    require(actual == [p*base for p in pts], 'derived original clocks differ')
    decoded,_ = training.bounded_process([str(ffmpeg),'-v','error','-nostdin','-threads','1',
        '-protocol_whitelist','file,pipe','-i',str(encoded),'-map','0:v:0','-an','-sn','-dn',
        '-fps_mode','passthrough','-pix_fmt','rgb24','-threads','1','-f','rawvideo','pipe:1'],len(rgb)+1,65536,30)
    require(decoded == rgb, 'derived decoded pixels differ')
    os.chmod(encoded,0o400)
    training.write_json(root/'derivation.json',{'encode_command':command,'probe_command':probe_command,
        'raw_rgb_sha256':hashlib.sha256(rgb).hexdigest(),'frame_count':len(pts),'original_timebase':str(base),
        'original_pts':pts,'encoded_timebase':str(actual_base),'encoded_pts':[int(f['best_effort_timestamp']) for f in probe['frames']],
        'exact_clock_and_pixel_correspondence':True,'source':identity(encoded),'qualification':'unqualified'})
    return encoded


def prepare(staged, candidate, output, worker, runner, runtime, ffmpeg, ffprobe):
    import numpy as np
    intake,intake_bytes = training.read_json(staged/'intake.json')
    manifest,manifest_bytes = training.read_json(staged/'manifest.json')
    training.validate_manifest(manifest)
    train,train_bytes = training.read_json(candidate/'training.json')
    split,split_bytes = training.read_json(candidate/'split.json')
    measured,_ = training.read_json(candidate/'holdout.json')
    require(intake['manifest_sha256'] == hashlib.sha256(manifest_bytes).hexdigest() and intake['split'] == 'training', 'training intake binding')
    require(train['intake_sha256'] == hashlib.sha256(intake_bytes).hexdigest()
        and measured['training_receipt_sha256'] == hashlib.sha256(train_bytes).hexdigest()
        and measured['split_receipt_sha256'] == hashlib.sha256(split_bytes).hexdigest(), 'frozen candidate receipt binding')
    require(split['dataset_sha256'] == intake['dataset_sha256'] == train['dataset_sha256'], 'candidate dataset binding')
    require({(e['id'],e['group']) for e in manifest['entries']} == {(e['id'],e['group']) for e in intake['entries']}, 'rights/group binding')
    training.validate_reference_offsets(manifest, intake)
    output.mkdir(mode=0o700,parents=False)
    try:
        copied = training.snapshot(staged/'dataset.npz',output/'dataset.npz',512*1024**2,512*1024**2)
        require(copied['sha256'] == intake['dataset_sha256'], 'staged dataset changed')
        x,y,pairs,ids = holdout.load_dataset(output/'dataset.npz')
        mask,_ = holdout.partition(intake,ids,split['heldout_groups'])
        require(int(mask.sum()) == split['heldout_samples'] == measured['heldout_samples'], 'heldout count differs')
        model_record = training.snapshot(candidate/'model.onnx',output/'model.onnx',256*1024**2,256*1024**2)
        require(model_record['sha256'] == train['model']['sha256'] == measured['model_sha256'], 'frozen model changed')
        model = output/'model.onnx'
        runtime_record = training.snapshot(runtime,output/'runtime.so',256*1024**2,256*1024**2)
        runtime = output/'runtime.so'
        artifacts = {name:identity(path) for name,path in [('worker',worker),('runner',runner),('runtime',runtime),('ffmpeg',ffmpeg),('ffprobe',ffprobe),('model',model)]}
        model_input = {'width':64,'height':64,'channels':6,'layout':'nchw','class_count':1,'decoder':'temporal_script_position'}
        profile = {'schema_version':1,'model':artifacts['model'],'runtime':artifacts['runtime'],
            'preprocessing':{'input':model_input,'resize':'nearest_stretch','color':'rgb_frame_pair','scale':[1/255]*6,'bias':[0]*6,'padding_byte':0},
            'provider':'cpu','minimum_memory_bytes':134217728,'minimum_output_bytes':1048576,'cpu_threads':2}
        cases=[]
        for entry in intake['entries']:
            selected = np.flatnonzero(ids == entry['id'])
            if entry['group'] not in split['heldout_groups']:
                continue
            name = entry['reference']['path']
            require(isinstance(name,str) and Path(name).name == name and len(name)<=128, 'contained staged reference required')
            ref_path = staged/name
            require(training.hash_file(ref_path) == entry['reference']['sha256'], 'original reference changed')
            actions = training.reference_actions(training.read_json(ref_path)[0])
            times = [a[0] for a in actions]
            base,origin = Fraction(entry['source_timebase']),Fraction(entry['source_origin'])
            cursor=0
            for clip in entry['clips']:
                count=clip['training_samples'];indices=selected[cursor:cursor+count];cursor+=count
                rgb,pts = reconstruct(x[indices],pairs[indices],clip,base)
                for index,pt in zip(indices,pts[1:]):
                    label=training.label_at(actions,times,pt*base-origin+training.reference_offset(entry))
                    require(label is not None and abs(label-float(y[index,0])) <= 1e-6, 'staged label differs from original supplied script')
                subject=f'development-clip-{len(cases):02}'
                root=output/subject;root.mkdir(mode=0o700)
                encoded=encode_clip(root,rgb,pts,base,ffmpeg,ffprobe)
                reference=reference_for_clip(actions,pts[0]*base,pts[-1]*base,origin-training.reference_offset(entry),subject)
                training.write_json(root/'reference.json',reference)
                tools={name:{'path':str(path),'identity':artifacts[name]} for name,path in [('ffmpeg',ffmpeg),('ffprobe',ffprobe)]}
                tools['onnx_runtime']={'path':str(runtime),'identity':artifacts['runtime']}
                input_value={'worker_path':str(worker),'worker_identity':artifacts['worker'],
                    'source':{'source_version':subject,'path':str(encoded),'identity':identity(encoded)},'tools':tools,
                    'settings':{'analysis_width':64,'analysis_height':64,'source_sample_stride':1,'axis':'stroke','vr_mode':False,'model_input':model_input,'inference_profile':profile},
                    'model':{'source_version':'frozen-development-candidate','path':str(model),'identity':artifacts['model']},'profile':profile,
                    'preset':'fast','budget':{'memory_bytes':1610612736,'output_bytes':16777216,'wall_time_ms':120000,'cpu_threads':2},
                    'subject':subject,'target':'declared-script','window_start_ns':'0','window_end_ns':reference['window']['end_ns'],
                    'evaluation_window_policy':'recorded_actions'}
                training.write_json(root/'input.json',input_value)
                cases.append({'id':subject,'declared_group':entry['group'],'input':str((root/'input.json').relative_to(output)),
                    'input_sha256':training.hash_file(root/'input.json'),'reference':str((root/'reference.json').relative_to(output)),
                    'reference_sha256':training.hash_file(root/'reference.json'),'expected_runs':count})
            require(cursor==len(selected), 'clip sample partition differs')
        require(1<=len(cases)<=32, 'development case count')
        training.write_json(output/'campaign.json',{'schema_version':1,'scope':'consumed live-reference development clips; not independent release evaluation',
            'qualification':'unqualified','training_authorized':True,'sterile_inputs_used':False,'model_retrained':False,
            'dataset_sha256':intake['dataset_sha256'],'parent_intake_sha256':hashlib.sha256(intake_bytes).hexdigest(),
            'model_sha256':train['model']['sha256'],'runner':{'path':str(runner),'identity':artifacts['runner']},'artifacts':artifacts,'cases':cases,
            'limitations':['Derived staged 64x64 stride-selected clips, not full-resolution original source generation.',
                'Group families and supplied script alignment remain independently unverified.','Complete original script edges only; no boundary interpolation or threshold tuning.']})
        print(json.dumps({'state':'prepared','cases':len(cases),'model_sha256':train['model']['sha256']}),flush=True)
    except BaseException as error:
        training.write_json(output/'failure.json',{'state':'failed','error_type':type(error).__name__,'message':str(error)[:2048],'qualification':'unqualified'})
        raise


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['staged-run','candidate-run','output','worker','runner','runtime']:
        parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    prepare(args.staged_run.resolve(strict=True),args.candidate_run.resolve(strict=True),args.output.absolute(),
        args.worker.resolve(strict=True),args.runner.resolve(strict=True),args.runtime.resolve(strict=True),Path('/usr/bin/ffmpeg'),Path('/usr/bin/ffprobe'))
