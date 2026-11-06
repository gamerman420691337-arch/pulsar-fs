"""Staged-pixel correspondence and independent reference clock regressions."""
import hashlib
import json
import importlib.util
from pathlib import Path
import tempfile
import unittest
import numpy as np
from fractions import Fraction

spec=importlib.util.spec_from_file_location('intake',Path(__file__).with_name('reference-worker-intake.py'))
intake=importlib.util.module_from_spec(spec);spec.loader.exec_module(intake)


class WorkerIntakeTests(unittest.TestCase):
    def test_declared_offset_maps_labels_and_complete_edges_to_the_same_clock(self):
        actions=[(0,0),(500,50),(1000,100),(1500,0)]
        origin=Fraction(7,3)
        for milliseconds in (250,-250,0):
            offset=intake.training.reference_offset({'reference_offset_ms':milliseconds})
            first=origin+Fraction(1,4);last=origin+Fraction(5,4)
            result=intake.reference_for_clip(actions,first,last,origin-offset,'declared-offset')
            self.assertTrue(result['strokes'])
            for edge in result['strokes']:
                for side in ('start','end'):
                    elapsed=Fraction(1,4)+Fraction(int(edge[side+'_ns']),10**9)
                    actual=intake.training.label_at(actions,[a[0] for a in actions],elapsed+offset)
                    self.assertAlmostEqual(actual,edge[side+'_position_ppm']/10**6,places=8)
        manifest={'entries':[{'id':'a','reference_offset_ms':250}]}
        intake.training.validate_reference_offsets(manifest,{'entries':[{'id':'a','reference_offset_ms':250}]})
        intake.training.validate_reference_offsets({'entries':[{'id':'a'}]},{'entries':[{'id':'a'}]})
        for entries in ([{'id':'a'}],[{'id':'a','reference_offset_ms':-250}],[],[{'id':'b','reference_offset_ms':250}],
                        [{'id':'a','reference_offset_ms':250}]*2):
            with self.assertRaises(ValueError):
                intake.training.validate_reference_offsets(manifest,{'entries':entries})

    def test_candidate_receipt_mismatch_refuses_before_creating_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);live=root/'Live-Reference';live.mkdir();staged=root/'staged';staged.mkdir();candidate=root/'candidate';candidate.mkdir()
            for name in ['a','b']:
                (live/name).write_bytes(b'fixture');(live/(name+'.json')).write_text('{}')
            manifest={'schema_version':1,'split':'training','authorization':'authored fixture only','input_root':str(live),
                'entries':[{'id':name,'group':name,'video':name,'reference':name+'.json','permitted_uses':['training']} for name in ['a','b']]}
            raw=json.dumps(manifest).encode();(staged/'manifest.json').write_bytes(raw)
            (staged/'intake.json').write_text(json.dumps({'manifest_sha256':hashlib.sha256(raw).hexdigest(),'split':'training'}))
            (candidate/'training.json').write_text(json.dumps({'intake_sha256':'0'*64}))
            (candidate/'split.json').write_text('{}');(candidate/'holdout.json').write_text('{}')
            output=root/'must-stay-absent'
            with self.assertRaises(ValueError):intake.prepare(staged,candidate,output,Path('/usr/bin/false'),Path('/usr/bin/false'),Path('/usr/bin/false'),Path('/usr/bin/ffmpeg'),Path('/usr/bin/ffprobe'))
            self.assertFalse(output.exists())

    def fixture(self):
        rgb=[np.full((3,64,64),v,np.uint8) for v in [0,20,40]]
        frames=np.stack([np.concatenate([rgb[0],rgb[1]]),np.concatenate([rgb[1],rgb[2]])])
        raw=b''.join(frame.transpose(1,2,0).tobytes() for frame in rgb)
        pts=[90000,105000,120000]
        return frames,np.asarray([[90000,105000],[105000,120000]],np.int64),{'decoded_pts':pts,'omitted_pairs':0,'raw_rgb_sha256':hashlib.sha256(raw).hexdigest()},raw

    def test_reconstruct_requires_original_pixel_and_clock_chain(self):
        frames,pairs,clip,raw=self.fixture()
        self.assertEqual(intake.reconstruct(frames,pairs,clip,Fraction(1,90000)),(raw,clip['decoded_pts']))
        broken=frames.copy();broken[1,0,0,0]=1
        with self.assertRaises(ValueError):intake.reconstruct(broken,pairs,clip,Fraction(1,90000))
        broken=pairs.copy();broken[1,0]+=1
        with self.assertRaises(ValueError):intake.reconstruct(frames,broken,clip,Fraction(1,90000))
        for change in [{'omitted_pairs':1},{'raw_rgb_sha256':'0'*64},{'decoded_pts':[90000,105000,120001]}]:
            with self.assertRaises(ValueError):intake.reconstruct(frames,pairs,{**clip,**change},Fraction(1,90000))

    def test_reference_edges_do_not_interpolate_clipped_boundaries_or_normalize_travel(self):
        result=intake.reference_for_clip([(0,20),(100,80),(200,80),(300,30),(400,90)],Fraction(1,20),Fraction(7,20),Fraction(0),'test')
        self.assertEqual(result['window'],{'start_ns':'0','end_ns':'300000000'})
        self.assertEqual(len(result['strokes']),1)
        edge=result['strokes'][0]
        self.assertEqual((edge['start_ns'],edge['end_ns']),('150000000','250000000'))
        self.assertEqual((edge['start_position_ppm'],edge['end_position_ppm']),(800000,300000))
        exact=intake.reference_for_clip([(0,0),(100,100),(200,0)],Fraction(1,3),Fraction(2,3),Fraction(1,2),'signed-origin')
        self.assertEqual(exact['strokes'][0]['start_ns'],'166666666')
        self.assertEqual(exact['strokes'][0]['end_ns'],'266666666')

    def test_native_nut_derivation_preserves_uneven_original_clocks_and_rgb(self):
        frames,pairs,clip,raw=self.fixture()
        pts=[90001,105123,120147]
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            encoded=intake.encode_clip(root,raw,pts,Fraction(1,90000),Path('/usr/bin/ffmpeg'),Path('/usr/bin/ffprobe'))
            self.assertTrue(encoded.is_file())
            proof,_=intake.training.read_json(root/'derivation.json')
            self.assertTrue(proof['exact_clock_and_pixel_correspondence'])
            self.assertEqual(proof['original_pts'],pts)
            self.assertEqual(proof['raw_rgb_sha256'],hashlib.sha256(raw).hexdigest())


if __name__=='__main__':unittest.main()
