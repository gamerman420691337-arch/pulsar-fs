"""Holdout leakage and allocation admission regressions; authored arrays only."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile
import io
import json
import hashlib
import subprocess
import sys
import numpy as np

spec=importlib.util.spec_from_file_location('holdout',Path(__file__).with_name('train-reference-holdout.py'))
holdout=importlib.util.module_from_spec(spec);spec.loader.exec_module(holdout)


class HoldoutTests(unittest.TestCase):
    def test_actual_training_and_onnx_evaluation_use_separate_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);live=root/'Live-Reference';live.mkdir()
            for name in ('a','b'):
                (live/name).write_bytes(name.encode())
                (live/(name+'.json')).write_text('{}')
            staged=root/'staged';staged.mkdir()
            manifest={'schema_version':1,'split':'training','authorization':'authored fixtures for regression','input_root':str(live),'entries':[{'id':name,'group':name,'video':name,'reference':name+'.json','permitted_uses':['training']} for name in ('a','b')]}
            manifest_bytes=json.dumps(manifest).encode();(staged/'manifest.json').write_bytes(manifest_bytes)
            np.savez(staged/'dataset.npz',frames=np.zeros((4,6,64,64),np.uint8),labels=np.asarray([[0],[0.2],[0.8],[1]],np.float32),source_pts=np.asarray([[0,1],[1,2],[0,1],[1,2]],np.int64),groups=np.asarray(['a','a','b','b']))
            intake={'schema_version':1,'split':'training','manifest_sha256':hashlib.sha256(manifest_bytes).hexdigest(),'dataset_sha256':holdout.training.hash_file(staged/'dataset.npz'),'samples':4,'entries':[{'id':name,'group':name,'source':{'sha256':name},'reference':{'sha256':'ref-'+name},'source_timebase':'1/30','clips':[{'training_samples':2}]} for name in ('a','b')]}
            (staged/'intake.json').write_text(json.dumps(intake))
            output=root/'run';command=[sys.executable,str(Path(holdout.__file__)),'--staged-run',str(staged),'--output',str(output),'--heldout-group','b','--epochs','1']
            completed=subprocess.run(command,capture_output=True,timeout=30)
            self.assertEqual(completed.returncode,0,completed.stderr.decode())
            result=json.loads((output/'holdout.json').read_text())
            self.assertEqual(result['optimizer_samples'],2);self.assertEqual(result['heldout_samples'],2)
            self.assertAlmostEqual(result['constant_training_mean'],0.1,places=6)
            self.assertAlmostEqual(result['aggregate']['training_mean_baseline_mae'],0.8,places=6)
            self.assertEqual(list(result['by_declared_group']),['b'])
            self.assertEqual(result['parity_samples'],2);self.assertLessEqual(result['maximum_onnx_absolute_error'],1e-5)
            self.assertFalse(result['sterile_evaluation_performed'])
            receipt=(output/'holdout.json').read_bytes()
            refused=subprocess.run(command,capture_output=True,timeout=10)
            self.assertNotEqual(refused.returncode,0);self.assertEqual((output/'holdout.json').read_bytes(),receipt)
            self.assertFalse((output/'failure.json').exists())

    def test_declared_family_stays_together_and_alias_leakage_refuses(self):
        def entry(name,group):
            return {'id':name,'group':group,'source':{'sha256':name},'reference':{'sha256':'ref-'+name},'clips':[{'training_samples':2}]}
        intake={'entries':[entry('a','family'),entry('b','family'),entry('c','other')]}
        ids=np.asarray(['a','c','b','a','b','c'])
        mask,groups=holdout.partition(intake,ids,['family'])
        self.assertEqual(mask.tolist(),[True,False,True,True,True,False])
        self.assertEqual(set(groups[mask]),{'family'})
        for selected in ([],['missing'],['family','other'],['family','family']):
            with self.assertRaises(ValueError):holdout.partition(intake,ids,selected)
        for key in ('source','reference'):
            original=intake['entries'][2][key]['sha256']
            intake['entries'][2][key]['sha256']=intake['entries'][0][key]['sha256']
            with self.assertRaises(ValueError):holdout.partition(intake,ids,['family'])
            intake['entries'][2][key]['sha256']=original
        with self.assertRaises(ValueError):holdout.partition(intake,np.asarray(['a','c','b']),['family'])

    def test_archive_types_ranges_and_headers_refuse_before_large_allocation(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'dataset.npz'
            arrays={'frames':np.zeros((2,6,64,64),np.uint8),'labels':np.zeros((2,1),np.float32),'source_pts':np.asarray([[0,1],[1,2]],np.int64),'groups':np.asarray(['a','b'])}
            np.savez(path,**arrays);self.assertEqual(len(holdout.load_dataset(path)[0]),2)
            for key,value in [('labels',np.full((2,1),np.nan,np.float32)),('groups',np.asarray(['a','b'],dtype=object)),('source_pts',np.asarray([[1,1],[1,2]],np.int64))]:
                altered=dict(arrays);altered[key]=value;np.savez(path,**altered)
                with self.assertRaises(ValueError):holdout.load_dataset(path)
            header=io.BytesIO();np.lib.format.write_array_header_1_0(header,{'descr':'|u1','fortran_order':False,'shape':(65536,6,64,64)})
            with zipfile.ZipFile(path,'w') as archive:
                archive.writestr('frames.npy',header.getvalue())
                for name in ('labels.npy','source_pts.npy','groups.npy'):archive.writestr(name,header.getvalue())
            with self.assertRaises(ValueError):holdout.load_dataset(path)


if __name__=='__main__':unittest.main()
