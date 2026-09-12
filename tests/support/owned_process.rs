//! Test oracle for the bounded owned-work contract, independent of PID reuse and zombies.
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Identity {
    pub pid: u32,
    pub parent: u32,
    pub group: u32,
    pub start: u64,
}
#[derive(Debug, PartialEq)]
pub enum State {
    Live(char),
    Zombie,
    Gone,
    Replaced,
}
impl Identity {
    pub fn parse(stat: &str) -> Result<Self> {
        let (pid, _) = stat.split_once(' ').context("process stat PID missing")?;
        let (_, fields) = stat.rsplit_once(')').context("process stat comm missing")?;
        let fields: Vec<_> = fields.split_whitespace().collect();
        ensure!(fields.len() >= 20, "process stat incomplete");
        Ok(Self {
            pid: pid.parse()?,
            parent: fields[1].parse()?,
            group: fields[2].parse()?,
            start: fields[19].parse()?,
        })
    }
    #[allow(dead_code)] // Also used by the standalone real-process qualifier.
    pub fn capture(pid: u32) -> Result<Self> {
        Self::parse(&std::fs::read_to_string(format!("/proc/{pid}/stat"))?)
    }
    pub fn state(&self) -> Result<State> {
        self.state_from_stat(std::fs::read_to_string(format!("/proc/{}/stat", self.pid)))
    }
    pub fn state_from_stat(&self, read: std::io::Result<String>) -> Result<State> {
        let stat = match read {
            Ok(stat) => stat,
            // A proc-stat descriptor opened while the process existed can
            // return ESRCH when the process is reaped before the read.
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) =>
            {
                return Ok(State::Gone);
            }
            Err(e) => return Err(e.into()),
        };
        let now = Self::parse(&stat)?;
        if now.start != self.start {
            return Ok(State::Replaced);
        }
        ensure!(
            now.group == self.group,
            "owned process changed group: {self:?} -> {now:?}"
        );
        let state = stat
            .rsplit_once(')')
            .unwrap()
            .1
            .trim_start()
            .chars()
            .next()
            .context("state missing")?;
        Ok(if matches!(state, 'Z' | 'X') {
            State::Zombie
        } else {
            State::Live(state)
        })
    }
    pub async fn stopped_by(&self, deadline: Instant) -> Result<()> {
        loop {
            let state = self.state()?;
            if !matches!(state, State::Live(_)) {
                ensure!(
                    Instant::now() <= deadline,
                    "termination was not observed within its absolute deadline: {self:?}"
                );
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "owned live work survived cancellation grace: {self:?}, state={state:?}"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    pub fn reaped(&self) -> Result<()> {
        let state = self.state()?;
        ensure!(
            matches!(state, State::Gone | State::Replaced),
            "direct child was not reaped: {self:?}, observed={state:?}"
        );
        Ok(())
    }
}
