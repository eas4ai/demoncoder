//! Real production BackendProcess and stdin-lease supervisor controls.
#[path = "support/owned_process.rs"]
mod owned_process;
#[allow(dead_code)]
#[path = "../src/adapters/process.rs"]
mod process;
use anyhow::{Context, Result, ensure};
use owned_process::{Identity, State};
use process::BackendProcess;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const ACTIVE: &str = "import os,json,time,pathlib; p=pathlib.Path('activity'); p.write_text('ready\\n'); print(json.dumps({'pid':os.getpid(),'supervisor':os.getppid()}),flush=True)\nwhile True:\n with p.open('a') as f:f.write('active\\n'); f.flush()\n time.sleep(.005)";

#[tokio::test]
async fn owned_descendant_negative_and_production_group_cleanup() -> Result<()> {
    for group_cleanup in [false, true] {
        let root = tempfile::tempdir()?;
        let mut process = BackendProcess::spawn_supervised(
            Path::new("/usr/bin/python3"),
            &["-u".into(), "-c".into(), ACTIVE.into()],
            root.path(),
            &[],
            Path::new(env!("CARGO_BIN_EXE_demoncoder")),
        )?;
        let ready = tokio::time::timeout(Duration::from_secs(3), process.receive()).await??;
        let backend = Identity::capture(ready["pid"].as_u64().context("backend pid")? as u32)?;
        let supervisor =
            Identity::capture(ready["supervisor"].as_u64().context("supervisor pid")? as u32)?;
        ensure!(
            backend.parent == supervisor.pid
                && backend.group == supervisor.pid
                && supervisor.group == supervisor.pid,
            "supervised group identity missing"
        );
        ensure!(
            matches!(backend.state()?, State::Live(_)),
            "backend must be live at control boundary"
        );
        let activity = root.path().join("activity");
        let before = std::fs::metadata(&activity)?.len();
        tokio::time::timeout(Duration::from_secs(1), async {
            while std::fs::metadata(&activity).unwrap().len() <= before {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        let deadline = Instant::now() + Duration::from_secs(2);
        if group_cleanup {
            process.stop().await?;
        } else {
            rustix::process::kill_process(
                rustix::process::Pid::from_raw(supervisor.pid as i32).unwrap(),
                rustix::process::Signal::KILL,
            )?;
        }
        let observed = backend.stopped_by(deadline).await;
        let after = std::fs::metadata(&activity)?.len();
        if !group_cleanup {
            ensure!(
                after > before,
                "negative control failed to perform harmless live work"
            );
        }
        // Explicit cleanup even when the negative control demonstrates failure.
        process.stop().await?;
        backend
            .stopped_by(Instant::now() + Duration::from_secs(2))
            .await?;
        supervisor.reaped()?;
        let final_size = std::fs::metadata(&activity)?.len();
        tokio::time::sleep(Duration::from_millis(30)).await;
        ensure!(
            std::fs::metadata(&activity)?.len() == final_size,
            "activity continued after cleanup"
        );
        eprintln!(
            "group_cleanup={group_cleanup}, backend={backend:?}, supervisor={supervisor:?}, activity_before={before}, activity_after={after}, oracle={observed:?}"
        );
        assert_eq!(
            observed.is_ok(),
            group_cleanup,
            "live negative must fail; production group cleanup must pass: {observed:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn process_oracle_distinguishes_live_zombie_and_replaced_identity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let script = "import os,json,time\npid=os.fork()\nif pid==0:os._exit(0)\nos.waitid(os.P_PID,pid,os.WEXITED|os.WNOWAIT)\nprint(json.dumps({'pid':os.getpid(),'zombie':pid,'supervisor':os.getppid()}),flush=True)\ntime.sleep(60)";
    let mut process = BackendProcess::spawn_supervised(
        Path::new("/usr/bin/python3"),
        &["-u".into(), "-c".into(), script.into()],
        root.path(),
        &[],
        Path::new(env!("CARGO_BIN_EXE_demoncoder")),
    )?;
    let ready = tokio::time::timeout(Duration::from_secs(3), process.receive()).await??;
    let live = Identity::capture(ready["pid"].as_u64().unwrap() as u32)
        .context("capture ready live parent")?;
    let zombie = Identity::capture(ready["zombie"].as_u64().unwrap() as u32)
        .context("capture waitid-confirmed unreaped zombie")?;
    let supervisor = Identity::capture(ready["supervisor"].as_u64().unwrap() as u32)
        .context("capture live supervisor")?;
    zombie
        .stopped_by(Instant::now() + Duration::from_secs(2))
        .await?;
    ensure!(
        zombie.state()? == State::Zombie && Path::new(&format!("/proc/{}", zombie.pid)).exists(),
        "intentionally unreaped zombie not observed"
    );
    ensure!(
        matches!(live.state()?, State::Live(_)),
        "live parent not observed"
    );
    let expired_rejected = zombie
        .stopped_by(Instant::now() - Duration::from_millis(1))
        .await
        .is_err();
    let mut replaced = live.clone();
    replaced.start += 1;
    ensure!(
        replaced.state()? == State::Replaced,
        "start-time identity control failed"
    );
    let mut wrong_group = live.clone();
    wrong_group.group += 1;
    ensure!(
        wrong_group.state().is_err(),
        "changed process group accepted"
    );
    process.stop().await?;
    live.stopped_by(Instant::now() + Duration::from_secs(2))
        .await?;
    supervisor.reaped()?;
    ensure!(
        expired_rejected,
        "expired observation bound was renewed for an inert process"
    );
    Ok(())
}

#[tokio::test]
async fn direct_reap_stat_race_and_late_observation_controls() -> Result<()> {
    use std::io::Read;
    let root = tempfile::tempdir()?;
    let mut process = BackendProcess::spawn(
        Path::new("/usr/bin/python3"),
        &["-u".into(), "-c".into(), ACTIVE.into()],
        root.path(),
        &[],
    )?;
    let ready = tokio::time::timeout(Duration::from_secs(3), process.receive()).await??;
    let owner = Identity::capture(ready["pid"].as_u64().unwrap() as u32)?;
    assert_eq!(owner.group, owner.pid);
    assert!(matches!(owner.state()?, State::Live(_)));
    let mut opened_stat = std::fs::File::open(format!("/proc/{}/stat", owner.pid))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    process.stop().await?;
    owner.reaped()?;
    let mut text = String::new();
    let read = opened_stat.read_to_string(&mut text).map(|_| text);
    assert_eq!(read.as_ref().unwrap_err().raw_os_error(), Some(3));
    eprintln!("actual opened proc stat across direct reap: {owner:?}, read={read:?}");
    assert_eq!(owner.state_from_stat(read)?, State::Gone);
    assert!(
        owner
            .state_from_stat(Err(std::io::Error::from_raw_os_error(13)))
            .is_err()
    );
    assert!(owner.state_from_stat(Ok("invalid".into())).is_err());
    owner.stopped_by(deadline).await?;
    tokio::time::sleep_until(tokio::time::Instant::from_std(
        deadline + Duration::from_millis(5),
    ))
    .await;
    owner.reaped()?; // A reap-only oracle falsely accepts this late observation.
    let late = owner.stopped_by(deadline).await;
    eprintln!("late observation of already reaped child: reap=Ok, complete_timing={late:?}");
    assert!(late.is_err());
    Ok(())
}
