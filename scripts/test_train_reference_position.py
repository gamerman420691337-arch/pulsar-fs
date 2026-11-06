"""Independent admission/clock regressions; authored frames only."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from fractions import Fraction

spec=importlib.util.spec_from_file_location('training',Path(__file__).with_name('train-reference-position.py'))
training=importlib.util.module_from_spec(spec);spec.loader.exec_module(training)


class TrainingTests(unittest.TestCase):
    def test_native_staging_preserves_manifest_bytes_and_declared_clock(self):
        import hashlib
        import numpy as np
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);live=root/'Live-Reference';live.mkdir()
            video=live/'v.mkv'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i','testsrc=size=64x64:rate=8:duration=3','-threads','1','-c:v','ffv1',str(video)],check=True)
            (live/'r.json').write_text(json.dumps({'actions':[{'at':t,'pos':t//30} for t in (0,750,1500,2250,3000)]}))
            for offset in (-250,250):
                manifest={'schema_version':2,'split':'training','authorization':'authored fixtures only','input_root':str(live),'entries':[{'id':'one','group':'one','video':'v.mkv','reference':'r.json','permitted_uses':['training'],'reference_offset_ms':offset}]}
                raw=json.dumps(manifest,separators=(',',':')).encode()
                path=root/'manifest.json';path.write_bytes(raw)
                output=root/str(offset);output.mkdir()
                training.stage(path,output,1,1,1,'/usr/bin/ffmpeg','/usr/bin/ffprobe')
                record=json.loads((output/'intake.json').read_text());entry=record['entries'][0]
                self.assertEqual((output/'manifest.json').read_bytes(),raw)
                self.assertEqual(record['manifest_sha256'],hashlib.sha256(raw).hexdigest())
                self.assertEqual(entry['reference_offset_ms'],offset)
                self.assertEqual(record['preprocessing']['orientation'],'encoded source; display rotation disabled')
                self.assertEqual(Fraction(entry['clips'][0]['start']),Fraction(1)-Fraction(offset,1000))
                training.validate_reference_offsets(manifest,record)
                with np.load(output/'dataset.npz',allow_pickle=False) as arrays:
                    for pair,label in zip(arrays['source_pts'],arrays['labels']):
                        script_time=int(pair[1])*Fraction(entry['source_timebase'])-Fraction(entry['source_origin'])+Fraction(offset,1000)
                        self.assertAlmostEqual(float(label[0]),float(script_time/3),places=6)

    def test_training_decode_ignores_display_rotation_like_packed_source_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);plain=root/'plain.mp4';rotated=root/'rotated.mp4'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i','testsrc2=size=96x64:rate=2:duration=2','-c:v','libx264','-pix_fmt','yuv420p','-threads','1',str(plain)],check=True)
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-display_rotation','90','-i',str(plain),'-c','copy',str(rotated)],check=True)
            probe=json.loads(subprocess.check_output(['/usr/bin/ffprobe','-v','error','-select_streams','v:0','-show_streams','-of','json',str(rotated)]))
            stream=probe['streams'][0];self.assertEqual((stream['width'],stream['height']),(96,64))
            self.assertTrue(any(abs(item.get('rotation',0))==90 for item in stream.get('side_data_list',[])))
            base=Fraction(stream['time_base'])
            expected,expected_pts,_=training.decode_clip(plain,0,1,1,'/usr/bin/ffmpeg',base,10)
            actual,actual_pts,command=training.decode_clip(rotated,0,1,1,'/usr/bin/ffmpeg',base,10)
            self.assertEqual(actual_pts,expected_pts)
            # Lossless remux changed display metadata, not packed video packets.
            self.assertTrue(actual==expected,'training rotated encoded-source pixels while the runtime keeps packed orientation')
            self.assertIn('-noautorotate',command)

    def test_labels_use_exact_time_and_never_extrapolate(self):
        actions=[(100,20),(200,80),(300,40)];times=[100,200,300]
        self.assertEqual(training.label_at(actions,times,Fraction(3,20)),0.5)
        self.assertEqual(training.label_at(actions,times,Fraction(1,5)),0.8)
        self.assertIsNone(training.label_at(actions,times,Fraction(99,1000)))
        self.assertIsNone(training.label_at(actions,times,Fraction(301,1000)))
        for actions in [[{'at':1,'pos':20},{'at':1,'pos':80}],[{'at':False,'pos':20},{'at':100,'pos':80}],[{'at':0,'pos':20},{'at':100,'pos':101}]]:
            with self.assertRaises(ValueError):training.reference_actions({'actions':actions})

    def test_declared_offset_is_exact_bounded_and_schema_fenced(self):
        actions=[(0,0),(1000,100)];times=[0,1000]
        self.assertEqual(training.label_at(actions,times,Fraction(1,4)+training.reference_offset({'reference_offset_ms':250})),0.5)
        self.assertIsNone(training.label_at(actions,times,Fraction(1,4)+training.reference_offset({'reference_offset_ms':-500})))
        for value in [True,1.5,'250',86400001,-86400001]:
            with self.assertRaises(ValueError):training.reference_offset({'reference_offset_ms':value})
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'Live-Reference';root.mkdir();(root/'v').write_bytes(b'v');(root/'r').write_bytes(b'r')
            entry={'id':'one','group':'one','video':'v','reference':'r','permitted_uses':['training'],'reference_offset_ms':250}
            m={'schema_version':2,'split':'training','authorization':'authored','input_root':str(root),'entries':[entry]};training.validate_manifest(m)
            m['schema_version']=1
            with self.assertRaises(ValueError):training.validate_manifest(m)

    def test_split_rights_and_symlink_boundaries(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'Live-Reference';root.mkdir();(root/'v').write_bytes(b'video');(root/'r').write_text('{}')
            manifest={'schema_version':1,'split':'training','authorization':'owner training permission','input_root':str(root),'entries':[{'id':'one','group':'one','video':'v','reference':'r','permitted_uses':['training']}]}
            training.validate_manifest(manifest)
            for field,value in [('split','heldout'),('split','development')]:
                altered=dict(manifest);altered[field]=value
                with self.assertRaises(ValueError):training.validate_manifest(altered)
            manifest['entries'][0]['permitted_uses']=['evaluation']
            with self.assertRaises(ValueError):training.validate_manifest(manifest)
            manifest['entries'][0]['permitted_uses']=['training'];(root/'link').symlink_to(root/'v');manifest['entries'][0]['video']='link'
            with self.assertRaises(ValueError):training.validate_manifest(manifest)

    def test_bounded_tool_refuses_oversized_output_and_reaps_timeout(self):
        with self.assertRaises(ValueError):training.bounded_process(['/usr/bin/python3','-c','print("x"*2000)'],100,100,3)
        with self.assertRaises(ValueError):training.bounded_process(['/usr/bin/python3','-c','import time;time.sleep(5)'],100,100,0.1)

    def test_existing_destination_is_never_changed_by_failure_handling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)/'Live-Reference';root.mkdir();(root/'v').write_bytes(b'video');(root/'r').write_text('{}')
            manifest=Path(temporary)/'manifest.json';manifest.write_text(json.dumps({'schema_version':1,'split':'training','authorization':'owner training permission','input_root':str(root),'entries':[{'id':'one','group':'one','video':'v','reference':'r','permitted_uses':['training']}]}))
            output=Path(temporary)/'existing';output.mkdir();(output/'keep').write_bytes(b'original')
            result=subprocess.run(['/usr/bin/python3',str(Path(__file__).with_name('train-reference-position.py')),'--manifest',str(manifest),'--output',str(output)],capture_output=True,timeout=5)
            self.assertNotEqual(result.returncode,0);self.assertEqual(list(output.iterdir()),[output/'keep']);self.assertEqual((output/'keep').read_bytes(),b'original')

    def test_decode_preserves_pts_without_fps_retiming(self):
        with tempfile.TemporaryDirectory() as temporary:
            video=Path(temporary)/'fixture.mkv'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i','testsrc=size=64x64:rate=8:duration=3','-threads','1','-c:v','ffv1',str(video)],check=True)
            rgb,pts,_=training.decode_clip(video,1,1,2,'/usr/bin/ffmpeg',Fraction(1,1000),5)
            self.assertEqual(pts,[1000,1250,1500,1750]);self.assertEqual(len(rgb),4*64*64*3)


if __name__=='__main__':unittest.main()
