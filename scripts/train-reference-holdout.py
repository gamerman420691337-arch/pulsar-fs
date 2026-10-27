"""Fixed live-reference development holdout from a hash-bound staged dataset.

Never reads sterile inputs or modifies originals. No production activation.
"""
import argparse
import importlib.util
import math
import os
from pathlib import Path
import sys
import zipfile
from fractions import Fraction

spec=importlib.util.spec_from_file_location('reference_training',Path(__file__).with_name('train-reference-position.py'))
training=importlib.util.module_from_spec(spec);spec.loader.exec_module(training)
require=training.require


def partition(intake,ids,heldout_groups):
    """Keep every sample of each declared group in exactly one partition."""
    import numpy as np
    entries=intake['entries'];by_id={entry['id']:entry for entry in entries}
    require(len(by_id)==len(entries) and len(entries)>=2,'invalid entry identities')
    available={entry['group'] for entry in entries};selected=set(heldout_groups)
    require(len(selected)==len(heldout_groups) and selected and selected<available,'holdout groups must be unique, known and leave training groups')
    require(set(ids)==set(by_id),'dataset entry identities differ')
    for entry in entries:
        require(int((ids==entry['id']).sum())==sum(clip['training_samples'] for clip in entry['clips']),'dataset sample counts differ')
    # Different aliases of identical source/reference bytes must not cross.
    for key in ('source','reference'):
        sides={}
        for entry in entries:
            digest=entry[key]['sha256'];side=entry['group'] in selected
            require(digest not in sides or sides[digest]==side,'identical bytes cross holdout boundary')
            sides[digest]=side
    groups=np.asarray([by_id[str(item)]['group'] for item in ids])
    mask=np.isin(groups,sorted(selected))
    require(mask.any() and (~mask).any(),'empty partition')
    return mask,groups


def load_dataset(path):
    """Bound archive and array headers before allocating; prohibit pickle."""
    import numpy as np
    expected={'frames.npy','labels.npy','source_pts.npy','groups.npy'}
    with zipfile.ZipFile(path) as archive:
        items=archive.infolist()
        require(len(items)==4 and {item.filename for item in items}==expected,'unexpected dataset members')
        require(sum(item.file_size for item in items)<=512*1024**2,'dataset inflated size exceeds admission')
        for item in items:
            require(not item.flag_bits & 1,'encrypted dataset refused')
            with archive.open(item) as member:
                version=np.lib.format.read_magic(member)
                require(version in ((1,0),(2,0)),'unsupported array header')
                reader=np.lib.format.read_array_header_1_0 if version==(1,0) else np.lib.format.read_array_header_2_0
                shape,fortran,dtype=reader(member,max_header_size=10000)
                require(not dtype.hasobject and not fortran and 1<=len(shape)<=4 and all(type(v) is int and 0<v<=65536 for v in shape),'invalid array header')
                require(math.prod(shape)*dtype.itemsize<=item.file_size,'array shape exceeds member bytes')
    with np.load(path,allow_pickle=False) as data:
        x=data['frames'];y=data['labels'];pts=data['source_pts'];ids=data['groups']
    count=len(x)
    require(2<=count<=65536 and x.shape==(count,6,64,64) and x.dtype==np.uint8,'frame contract differs')
    require(y.shape==(count,1) and y.dtype==np.float32 and np.isfinite(y).all() and ((y>=0)&(y<=1)).all(),'label contract differs')
    require(pts.shape==(count,2) and pts.dtype==np.int64 and (pts[:,1]>pts[:,0]).all(),'source timestamp contract differs')
    require(ids.shape==(count,) and ids.dtype.kind=='U' and ids.dtype.itemsize<=256,'sample identity contract differs')
    return x,y,pts,ids


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged-run',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--heldout-group',action='append',required=True)
    parser.add_argument('--epochs',type=int,default=10)
    parser.add_argument('--seed',type=int,default=20261004)
    args=parser.parse_args()
    require(1<=args.epochs<=100,'epoch admission')
    intake,intake_bytes=training.read_json(args.staged_run/'intake.json')
    manifest,manifest_bytes=training.read_json(args.staged_run/'manifest.json')
    import hashlib
    require(intake['schema_version']==1 and intake['split']=='training' and manifest['split']=='training' and hashlib.sha256(manifest_bytes).hexdigest()==intake['manifest_sha256'],'training intake identity differs')
    require({(e['id'],e['group']) for e in intake['entries']}=={(e['id'],e['group']) for e in manifest['entries']} and all(e['permitted_uses']==['training'] for e in manifest['entries']),'training rights/group declarations differ')
    training.validate_manifest(manifest)
    training.validate_reference_offsets(manifest, intake)
    args.output.mkdir(mode=0o700,parents=False)
    try:
        copied=training.snapshot(args.staged_run/'dataset.npz',args.output/'dataset.npz',512*1024**2,512*1024**2)
        require(copied['sha256']==intake['dataset_sha256'],'staged dataset hash differs')
        x,y,pts,ids=load_dataset(args.output/'dataset.npz')
        require(len(x)==intake['samples'],'intake sample count differs')
        mask,groups=partition(intake,ids,args.heldout_group)
        for entry in intake['entries']:
            base=Fraction(entry['source_timebase'])
            require(base>0 and all(int(b)-int(a)>0 and (int(b)-int(a))*base<=Fraction(1,2) for a,b in pts[ids==entry['id']]),'source timebase/pair gap differs')
        training.write_json(args.output/'intake.json',intake)
        training.write_json(args.output/'split.json',{'schema_version':1,'scope':'live-reference development holdout','dataset_sha256':copied['sha256'],'parent_intake_sha256':hashlib.sha256(intake_bytes).hexdigest(),'training_groups':sorted(set(groups[~mask])),'heldout_groups':sorted(set(groups[mask])),'training_samples':int((~mask).sum()),'heldout_samples':int(mask.sum()),'selection':'explicit group list before fitting; fixed epochs; no validation-based selection','family_identity':'owner manifest declarations; independently unverified','sterile_inputs_used':False,'qualification':'unqualified'})
        training.write_json(args.output/'invocation.json',{'schema_version':1,'argv':sys.argv,'python_version':sys.version,'seed':args.seed,'epochs':args.epochs,'trainer_sha256':training.hash_file(Path(training.__file__)),'holdout_driver_sha256':training.hash_file(Path(__file__)),'network_required':False})
        training.train(x[~mask],y[~mask],args.output,intake,args.epochs,args.seed,(x[mask],y[mask],groups[mask]))
    except BaseException as error:
        training.write_json(args.output/'failure.json',{'schema_version':1,'state':'failed','error_type':type(error).__name__,'message':str(error)[:2048],'qualification':'unqualified'})
        raise


if __name__=='__main__':
    main()
