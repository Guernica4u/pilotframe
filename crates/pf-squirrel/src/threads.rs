//! Script threads and signals, as launcher.dll runs them
//! (docs/port/scripts.md, "Threads and signals"):
//! - `SpinOff(f, ...)` starts `f` as a thread and runs it at once until it
//!   first waits; its `this` is `SpinOff`'s `this`. The `thread` keyword
//!   compiles to a call of it (`waitthread` to `SpinOffAndWaitFor`,
//!   `waitthreadsolo` to `SpinOffAndWaitForSolo`, `delaythread(d)` to
//!   `SpinOffDelayed(d, f, ...)`, `wait t` to `Wait(t)`).
//! - Waiting threads sit in buckets sorted by wake time (first in, first out
//!   within a bucket). Each frame, due buckets move onto the ready list, and
//!   the ready list runs in rounds until it's empty; then the threads that
//!   called `WaitEndFrame` run.
//! - A thread woken outside a frame (from engine code) runs at the start of
//!   the next frame; one woken during a frame runs in the next round of the
//!   same frame.
//! - `Signal(owner, name, results)` first ends the threads that called
//!   `EndSignal(owner, name)`, then wakes the ones in `WaitSignal(owner,
//!   name, ...)`, whose call returns `results` (with `results.signal` set).

use std::collections::VecDeque;

use crate::table::Table;
use crate::value::*;
use crate::vm::{Exec, NativeError, NativeResult, Outcome, Start, Vm, throw};

/// The yield of a script thread that has stopped for now.
pub(crate) const SUSPEND: NativeError = NativeError::Suspend;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Where {
    /// Not queued anywhere (running, or waiting only on signals).
    Nowhere,
    Bucket,
    Ready,
    /// Woken outside a frame: runs at the start of the next one.
    NextFrame,
    EndFrame,
}

struct ScriptThread {
    /// Its stack (None while it runs).
    exec: Option<Exec>,
    started: bool,
    dead: bool,
    /// End it when it next stops (an `EndSignal` on an owner already gone).
    end_pending: bool,
    /// The thread waiting for this one (`waitthread`).
    parent: Option<usize>,
    /// The thread this one waits for.
    child: Option<usize>,
    /// Ending this thread ends its child too (`waitthread`, not
    /// `waitthreadsolo`).
    ends_child: bool,
    /// What the call it stopped in returns when it carries on.
    resume: Value,
    /// `OnThreadEnd` functions, in order.
    on_end: Vec<Value>,
    place: Where,
    /// Its stack is in use right now (perhaps further up, under a thread it
    /// started), so its slot can't be reused yet.
    running: bool,
    /// Bumped whenever it's queued or unqueued, so stale queue entries are
    /// skipped (the game unlinks it from its list instead).
    ticket: u64,
}

type Entry = (usize, u64);

#[derive(Default)]
pub(crate) struct Threads {
    slots: Vec<Option<ScriptThread>>,
    free: Vec<usize>,
    pub(crate) current: Option<usize>,
    /// (wake time, threads) in time order.
    buckets: Vec<(f32, VecDeque<Entry>)>,
    ready: VecDeque<Entry>,
    next_frame: VecDeque<Entry>,
    end_frame: VecDeque<Entry>,
    in_frame: bool,
    /// The current frame's time.
    pub(crate) time: f32,
    signals: Table,
    /// (owner, signal, thread) for `WaitSignal`, in the order made.
    waits: Vec<(Value, Value, usize)>,
    /// The same for `EndSignal`.
    ends: Vec<(Value, Value, usize)>,
}

impl Threads {
    fn get(&self, id: usize) -> Option<&ScriptThread> {
        self.slots.get(id).and_then(Option::as_ref)
    }

    fn get_mut(&mut self, id: usize) -> Option<&mut ScriptThread> {
        self.slots.get_mut(id).and_then(Option::as_mut)
    }

    fn alive(&self, id: usize) -> bool {
        self.get(id).is_some_and(|t| !t.dead)
    }

    /// Take a thread off whatever list or bucket it's on.
    fn unqueue(&mut self, id: usize) {
        if let Some(t) = self.get_mut(id) {
            t.place = Where::Nowhere;
            t.ticket += 1;
        }
    }

    fn queue(&mut self, id: usize, place: Where) {
        let Some(t) = self.get_mut(id) else { return };
        t.ticket += 1;
        t.place = place;
        let e = (id, t.ticket);
        match place {
            Where::Ready => self.ready.push_back(e),
            Where::NextFrame => self.next_frame.push_back(e),
            Where::EndFrame => self.end_frame.push_back(e),
            Where::Bucket | Where::Nowhere => {}
        }
    }

    /// Wait until `time` (appended to that time's bucket).
    fn queue_at(&mut self, id: usize, time: f32) {
        let Some(t) = self.get_mut(id) else { return };
        t.ticket += 1;
        t.place = Where::Bucket;
        let e = (id, t.ticket);
        let i = self.buckets.partition_point(|(bt, _)| *bt < time);
        if self.buckets.get(i).is_some_and(|(bt, _)| *bt == time) {
            self.buckets[i].1.push_back(e);
        } else {
            self.buckets.insert(i, (time, VecDeque::from([e])));
        }
    }

    fn is_live_entry(&self, (id, ticket): Entry, place: Where) -> bool {
        self.get(id)
            .is_some_and(|t| t.ticket == ticket && t.place == place && !t.dead)
    }

    fn remove_waits(&mut self, id: usize) {
        self.waits.retain(|(_, _, t)| *t != id);
    }

    fn remove_ends(&mut self, id: usize) {
        self.ends.retain(|(_, _, t)| *t != id);
    }

    /// Wake a waiting thread: off its bucket and signal waits, onto the
    /// ready list during a frame or the next frame's list outside one.
    fn wake(&mut self, id: usize) {
        self.unqueue(id);
        self.remove_waits(id);
        let place = if self.in_frame {
            Where::Ready
        } else {
            Where::NextFrame
        };
        self.queue(id, place);
    }
}

fn same(a: &Value, b: &Value) -> bool {
    a.raw_eq(b)
}

impl Vm {
    fn new_thread(&mut self, exec: Exec) -> usize {
        let t = ScriptThread {
            exec: Some(exec),
            started: false,
            dead: false,
            end_pending: false,
            parent: None,
            child: None,
            ends_child: false,
            resume: Value::Null,
            on_end: Vec::new(),
            place: Where::Nowhere,
            running: false,
            ticket: 0,
        };
        let th = &mut self.threads;
        match th.free.pop() {
            Some(i) => {
                th.slots[i] = Some(t);
                i
            }
            None => {
                th.slots.push(Some(t));
                th.slots.len() - 1
            }
        }
    }

    /// Run a thread until it stops. False if it's finished (or failed).
    fn resume_thread(&mut self, id: usize) -> bool {
        let Some(t) = self.threads.get_mut(id) else {
            return false;
        };
        if t.dead {
            return false;
        }
        let Some(mut ex) = t.exec.take() else {
            return false;
        };
        let first = !t.started;
        t.started = true;
        t.running = true;
        let resume = std::mem::take(&mut t.resume);
        let prev = self.threads.current.replace(id);
        let outcome = if first {
            let argc = ex.stack.len() - 2;
            match self.begin_call(&mut ex, 0, argc) {
                Ok(Start::Done(v)) => Ok(Outcome::Returned(v)),
                Ok(Start::Suspend) => Ok(Outcome::Suspended),
                Ok(Start::Frame) => self.run(&mut ex, 0),
                Err(e) => Err(e),
            }
        } else {
            ex.stack.push(resume);
            self.run(&mut ex, 0)
        };
        self.threads.current = prev;
        let alive = match outcome {
            Ok(Outcome::Suspended) => true,
            Ok(Outcome::Returned(_)) => false,
            Err(e) => {
                // An error ends the thread; the game prints it and carries on.
                self.report_error(&e);
                false
            }
        };
        let Some(t) = self.threads.get_mut(id) else {
            return false;
        };
        t.running = false;
        if alive && !t.dead && !t.end_pending {
            t.exec = Some(ex);
            true
        } else {
            // It ended: its OnThreadEnd functions run first.
            self.run_on_end(id);
            self.free_if_done(id);
            false
        }
    }

    /// Reuse a dead thread's slot once nothing is running on it.
    fn free_if_done(&mut self, id: usize) {
        if self.threads.get(id).is_some_and(|t| t.dead && !t.running) {
            self.threads.slots[id] = None;
            self.threads.free.push(id);
        }
    }

    fn run_on_end(&mut self, id: usize) {
        let Some(t) = self.threads.get_mut(id) else {
            return;
        };
        let funcs = std::mem::take(&mut t.on_end);
        if funcs.is_empty() {
            return;
        }
        // They run outside any thread, so they can't wait.
        let prev = self.threads.current.take();
        let root = Value::Table(self.root.clone());
        for f in funcs {
            if let Err(e) = self.call(&f, root.clone(), &[]) {
                self.report_error(&e);
            }
        }
        self.threads.current = prev;
    }

    /// End a thread (finished, or ended by a signal).
    fn end_thread(&mut self, id: usize) {
        let Some(t) = self.threads.get_mut(id) else {
            return;
        };
        if t.dead {
            return;
        }
        t.dead = true;
        let (parent, child, ends_child) = (t.parent.take(), t.child.take(), t.ends_child);
        t.ends_child = false;
        // The thread waiting for this one carries on.
        if let Some(p) = parent {
            if let Some(pt) = self.threads.get_mut(p) {
                pt.child = None;
            }
            self.threads.wake(p);
        }
        // A `waitthread` child ends with it.
        if let Some(c) = child {
            if let Some(ct) = self.threads.get_mut(c) {
                ct.parent = None;
            }
            if ends_child {
                self.end_thread(c);
            }
        }
        self.threads.unqueue(id);
        self.threads.remove_waits(id);
        self.threads.remove_ends(id);
        self.run_on_end(id);
        if let Some(t) = self.threads.get_mut(id) {
            t.exec = None;
        }
        self.free_if_done(id);
    }

    /// Run one frame of script threads at game time `now` (seconds).
    pub fn run_frame(&mut self, now: f32) {
        let th = &mut self.threads;
        th.time = now;
        th.in_frame = true;
        std::mem::swap(&mut th.ready, &mut th.next_frame);
        for e in th.ready.iter_mut() {
            if let Some(t) = th.slots.get_mut(e.0).and_then(Option::as_mut)
                && t.ticket == e.1
            {
                t.place = Where::Ready;
            }
        }
        if !th.ready.iter().any(|&e| th.is_live_entry(e, Where::Ready)) {
            self.promote_end_frame();
        }
        // Due buckets move onto the ready list, in time order.
        loop {
            let th = &mut self.threads;
            match th.buckets.first() {
                Some((t, _)) if *t - now <= 0.0 => {}
                _ => break,
            }
            let (_, entries) = th.buckets.remove(0);
            for e in entries {
                if th.is_live_entry(e, Where::Bucket) {
                    // A WaitSignalTimeout that timed out stops waiting.
                    th.remove_waits(e.0);
                    th.queue(e.0, Where::Ready);
                }
            }
        }
        // Rounds until nothing is ready; then the end-of-frame threads.
        let mut rounds = 0;
        loop {
            if !self
                .threads
                .ready
                .iter()
                .any(|&e| self.threads.is_live_entry(e, Where::Ready))
            {
                self.threads.ready.clear();
                self.promote_end_frame();
            }
            let round = std::mem::take(&mut self.threads.ready);
            if round.is_empty() {
                break;
            }
            rounds += 1;
            if rounds > 10_000 {
                // Not in the game: a guard against a thread calling
                // WaitEndFrame forever in one frame.
                break;
            }
            for e in round {
                if !self.threads.is_live_entry(e, Where::Ready) {
                    continue;
                }
                self.threads.unqueue(e.0);
                if !self.resume_thread(e.0) {
                    self.end_thread(e.0);
                }
            }
        }
        self.threads.in_frame = false;
    }

    /// The end-of-frame threads become the ready list.
    fn promote_end_frame(&mut self) {
        let th = &mut self.threads;
        th.ready.clear();
        for e in std::mem::take(&mut th.end_frame) {
            if th.is_live_entry(e, Where::EndFrame) {
                th.queue(e.0, Where::Ready);
            }
        }
    }

    /// `SpinOff` and its variants. `mode`: 0 don't wait, 1 wait for it
    /// (`waitthread`), 2 wait without passing end signals on
    /// (`waitthreadsolo`).
    fn spin_off(
        &mut self,
        this: Value,
        f: Value,
        args: &[Value],
        delay: f32,
        mode: u8,
    ) -> NativeResult {
        let parent = self.threads.current;
        if mode != 0 && parent.is_none() {
            return Err(throw(
                "Called SpinOffAndWaitFor() from script that was not started with SpinOff()",
            ));
        }
        if !matches!(f, Value::Closure(_) | Value::Native(_)) {
            return Err(throw(format!(
                "SpinOff: Expected function, found {}",
                f.type_name()
            )));
        }
        let mut ex = Exec::new(true);
        ex.stack.push(f);
        ex.stack.push(this);
        ex.stack.extend_from_slice(args);
        let id = self.new_thread(ex);
        let alive = if delay <= 0.0 {
            if mode == 1
                && let Some(p) = parent.and_then(|p| self.threads.get_mut(p))
            {
                p.child = Some(id);
                p.ends_child = true;
            }
            let alive = self.resume_thread(id);
            if !alive {
                if mode == 1
                    && let Some(p) = parent.and_then(|p| self.threads.get_mut(p))
                {
                    p.child = None;
                    p.ends_child = false;
                }
                self.end_thread(id);
            }
            alive
        } else {
            let at = self.threads.time + delay;
            self.threads.queue_at(id, at);
            true
        };
        // The new thread may have ended this one.
        if let Some(p) = parent
            && !self.threads.alive(p)
        {
            return Err(SUSPEND);
        }
        if mode != 0
            && alive
            && let Some(p) = parent
        {
            if let Some(pt) = self.threads.get_mut(p) {
                pt.child = Some(id);
                pt.ends_child = mode != 2;
            }
            if let Some(ct) = self.threads.get_mut(id) {
                ct.parent = Some(p);
            }
            return Err(SUSPEND);
        }
        Ok(Value::Null)
    }

    /// Start a script thread from engine code (as `SpinOff` does).
    pub fn spin_off_from_code(
        &mut self,
        f: &Value,
        this: Value,
        args: &[Value],
    ) -> Result<(), Value> {
        match self.spin_off(this, f.clone(), args, 0.0, 0) {
            Ok(_) | Err(NativeError::Suspend) => Ok(()),
            Err(NativeError::Throw(e)) => Err(e),
        }
    }

    fn current_or(&self, what: &str) -> Result<usize, NativeError> {
        self.threads.current.ok_or_else(|| {
            throw(format!(
                "Called {what}() from script that was not started with SpinOff()"
            ))
        })
    }

    fn check_signal(&self, sig: &Value) -> Result<(), NativeError> {
        match sig {
            Value::Str(s) if self.threads.signals.contains(sig) => {
                let _ = s;
                Ok(())
            }
            Value::Str(s) => Err(throw(format!(
                "\"{}\" is not a registered signal",
                s.to_string_lossy()
            ))),
            other => Err(throw(format!(
                "Type {} is not a valid signal",
                other.type_name()
            ))),
        }
    }

    /// `Signal(owner, signal[, results])`.
    pub fn signal(&mut self, owner: &Value, sig: &Value, results: Option<Value>) -> NativeResult {
        if owner.object_id().is_none() {
            return Err(throw(format!(
                "Cannot signal on object of type {}.",
                owner.type_name()
            )));
        }
        self.check_signal(sig)?;
        let results = match results {
            None | Some(Value::Null) => Value::Table(new_ref(Table::new())),
            Some(t @ Value::Table(_)) => t,
            Some(_) => return Err(throw("Signal: third argument isn't a table")),
        };
        if let Value::Table(t) = &results {
            t.borrow_mut().insert(Value::str("signal"), sig.clone());
        }
        // First end the threads with a matching EndSignal...
        let to_end: Vec<usize> = self
            .threads
            .ends
            .iter()
            .filter(|(o, s, _)| same(o, owner) && same(s, sig))
            .map(|(_, _, t)| *t)
            .collect();
        for t in to_end {
            self.end_thread(t);
        }
        // ...then wake the ones waiting for it.
        let to_wake: Vec<usize> = self
            .threads
            .waits
            .iter()
            .filter(|(o, s, _)| same(o, owner) && same(s, sig))
            .map(|(_, _, t)| *t)
            .collect();
        for t in to_wake {
            if let Some(th) = self.threads.get_mut(t) {
                th.resume = results.clone();
            }
            self.threads.wake(t);
        }
        if let Some(cur) = self.threads.current
            && !self.threads.alive(cur)
        {
            return Err(SUSPEND);
        }
        Ok(Value::Null)
    }
}

/// The thread natives, registered in the root table.
pub(crate) fn install(vm: &mut Vm) {
    vm.register("SpinOff", -2, |vm, a| {
        vm.spin_off(a[0].clone(), a[1].clone(), &a[2..], 0.0, 0)
    });
    vm.register("SpinOffAndWaitFor", -2, |vm, a| {
        vm.spin_off(a[0].clone(), a[1].clone(), &a[2..], 0.0, 1)
    });
    vm.register("SpinOffAndWaitForSolo", -2, |vm, a| {
        vm.spin_off(a[0].clone(), a[1].clone(), &a[2..], 0.0, 2)
    });
    vm.register("SpinOffDelayed", -3, |vm, a| {
        let delay = match a[1] {
            Value::Int(i) => i as f32,
            Value::Float(x) => x,
            _ => return Err(throw("SpinOffDelayed: second argument must be a float")),
        };
        vm.spin_off(a[0].clone(), a[2].clone(), &a[3..], delay, 0)
    });
    vm.register("Wait", 0, |vm, a| {
        if a.len() != 2 {
            return Err(throw("Wrong number of arguments to Wait( time )"));
        }
        let t = match a[1] {
            Value::Int(i) => i as f32,
            Value::Float(x) => x,
            ref other => {
                return Err(throw(format!(
                    "Wait( time ): expected float, found {}",
                    other.type_name()
                )));
            }
        };
        let id = vm.current_or("Wait")?;
        let at = vm.threads.time + t;
        vm.threads.queue_at(id, at);
        Err(SUSPEND)
    });
    vm.register("WaitEndFrame", 1, |vm, _| {
        let id = vm.current_or("WaitEndFrame")?;
        vm.threads.queue(id, Where::EndFrame);
        Err(SUSPEND)
    });
    vm.register("RegisterSignal", 2, |vm, a| match &a[1] {
        Value::Str(_) => {
            vm.threads.signals.insert(a[1].clone(), Value::Bool(true));
            Ok(Value::Null)
        }
        _ => Err(throw("RegisterSignal: must specify a string")),
    });
    vm.register("IsValidSignal", 2, |vm, a| {
        Ok(Value::Bool(vm.threads.signals.contains(&a[1])))
    });
    vm.register("Signal", 0, |vm, a| {
        if a.len() < 3 {
            return Err(throw(
                "Wrong number of arguments to Signal( owner, signalID [, resultTable] )",
            ));
        }
        if a.len() > 4 {
            return Err(throw(
                "Too many arguments to Signal( owner, signalID [, resultTable] )",
            ));
        }
        if a[1].is_null() {
            return Err(throw("Owner is NULL!\n"));
        }
        vm.signal(&a[1], &a[2], a.get(3).cloned())
    });
    vm.register("WaitSignal", -3, |vm, a| {
        wait_signal(vm, &a[1], &a[2..], None)
    });
    vm.register("WaitSignalOnDeadEnt", -3, |vm, a| {
        wait_signal(vm, &a[1], &a[2..], None)
    });
    vm.register("WaitSignalTimeout", -4, |vm, a| {
        let t = match a[2] {
            Value::Int(i) => i as f32,
            Value::Float(x) => x,
            _ => {
                return Err(throw(
                    "WaitSignalTimeout: expected float for second parameter",
                ));
            }
        };
        wait_signal(vm, &a[1], &a[3..], Some(t))
    });
    vm.register("EndSignal", -3, |vm, a| {
        let id = vm.current_or("EndSignal")?;
        for sig in &a[2..] {
            vm.check_signal(sig)?;
            vm.threads.ends.push((a[1].clone(), sig.clone(), id));
        }
        Ok(Value::Null)
    });
    vm.register("OnThreadEnd", -2, |vm, a| {
        let id = vm.current_or("OnThreadEnd")?;
        let Value::Closure(c) = &a[1] else {
            return Err(throw("OnThreadEnd(): expected closure for first argument"));
        };
        // Bound to the caller's `this`.
        let f = Value::Closure(std::rc::Rc::new(Closure {
            proto: c.proto.clone(),
            free: c.free.clone(),
            defaults: c.defaults.clone(),
            env: Some(a[0].clone()),
        }));
        if let Some(t) = vm.threads.get_mut(id) {
            t.on_end.push(f);
        }
        Ok(Value::Null)
    });
}

fn wait_signal(vm: &mut Vm, owner: &Value, sigs: &[Value], timeout: Option<f32>) -> NativeResult {
    let id = vm.current_or("WaitSignal")?;
    for sig in sigs {
        vm.check_signal(sig)?;
    }
    for sig in sigs {
        vm.threads.waits.push((owner.clone(), sig.clone(), id));
    }
    if let Some(t) = timeout {
        let at = vm.threads.time + t;
        vm.threads.queue_at(id, at);
    }
    Err(SUSPEND)
}
