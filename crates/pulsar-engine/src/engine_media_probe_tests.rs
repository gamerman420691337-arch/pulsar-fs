#[test]
fn media_probe_refuses_scope_revision_and_nonmedia_before_workers_or_jobs() {
    for case in 0..5 {
        let f=fixture();let path=f.dir.path().join("media.bin");fs::write(&path,b"authored opaque media").unwrap();
        let ResponseBody::Source {source_version}=f.engine.handle(request(&f.session,Some(&f.project),None,Command::ImportSource {path})).result.unwrap() else {panic!("source");};
        let mut query=request(&f.session,Some(&f.project),Some(0),Command::ProbeMedia {source_version:source_version.clone()});
        match case {
            0=>query.expected_revision=None,
            1=>query.expected_revision=Some(RevisionId::new(1)),
            2=> {let session=pair(&f.engine,"ungranted probe");query=request(&session,Some(&f.project),Some(0),query.command.clone());},
            3=>query.command=Command::ProbeMedia {source_version:SourceVersionId::new("unknown").unwrap()},
            _=> {f.engine.db.lock().unwrap().execute("UPDATE sources SET kind='funscript' WHERE version=?1",[source_version.as_str()]).unwrap();},
        }
        let before=serde_json::to_vec(&project_snapshot(&f.engine.db.lock().unwrap(),&f.project).unwrap()).unwrap();
        assert!(f.engine.handle(query.clone()).result.is_err());assert!(f.engine.handle(query).result.is_err());
        let db=f.engine.db.lock().unwrap();assert_eq!(serde_json::to_vec(&project_snapshot(&db,&f.project).unwrap()).unwrap(),before);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM jobs",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(fs::read_dir(f.engine.config.state_dir.join("attempts")).unwrap().count(),0);
        assert!(f.engine.workers.lock().unwrap().is_empty());
    }
}
#[test]
fn media_probe_reservation_protects_source_from_eviction() {
    let f=fixture();let path=f.dir.path().join("media.bin");fs::write(&path,b"authored media").unwrap();
    let ResponseBody::Source {source_version}=f.engine.handle(request(&f.session,Some(&f.project),None,Command::ImportSource {path})).result.unwrap() else {panic!("source");};
    let key=format!("probe:{}:{}:{}:request",f.project,f.session,source_version);
    f.engine.workers.lock().unwrap().insert(key.clone(),Arc::new(AtomicBool::new(false)));
    let response=f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::EvictSource {source_version:source_version.clone()}));
    assert_eq!(response.result.unwrap_err().code,ErrorCode::Forbidden);
    assert_eq!(f.engine.db.lock().unwrap().query_row("SELECT evicted FROM sources WHERE version=?1",[source_version.as_str()],|r|r.get::<_,i64>(0)).unwrap(),0);
    f.engine.workers.lock().unwrap().remove(&key);
}
