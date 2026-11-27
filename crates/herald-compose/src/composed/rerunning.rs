//! `herald rerun` through a composed herald: a rule's plans listed, or
//! applied by an operator signed in at the forge (ADR-0068).

use herald_flows::{
    Admission, Applying, PlansAsked, RerunApplied, RerunPlans, SigningIn, Stop, rerun_apply,
    rerun_plans,
};
use herald_ports::{Base, Keyring, Store, Timer};

use super::{Composed, ServingOn, TIMEOUT_MS, Through};
use crate::counted::Counted;
use crate::host::Host;

/// The plans of a rule, or of every rule that plans re-runs, as printed.
struct Plans<'r>(PlansAsked<'r>);

impl<H: Host> Through<H> for Plans<'_> {
    type Done = Result<RerunPlans, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        rerun_plans((serving.config, serving.connected, serving.ports), self.0).await
    }
}

/// The runs an operator named, applied.
struct Apply<'a, H: Host, T> {
    applying: &'a Applying<'a, Counted<H::Http>, T>,
    planning: PlansAsked<'a>,
    chosen: &'a [(String, String)],
}

impl<H: Host, T: Timer> Through<H> for Apply<'_, H, T> {
    type Done = Result<RerunApplied, Stop>;

    async fn through<S: Store, K: Keyring>(
        self,
        serving: &ServingOn<'_, H, S, K>,
        _: &str,
    ) -> Self::Done {
        rerun_apply(
            (serving.config, serving.connected, serving.ports),
            self.applying,
            (self.planning, self.chosen),
        )
        .await
    }
}

impl<'o, H: Host + 'o> Composed<'o, H>
where
    H: Send,
    H::Http: Send + 'static,
    H::Clock: Send + 'static,
    H::Sql: 'static,
    H::Kv: 'static,
{
    /// The plans of the rule `rule`, or of every rule that plans re-runs,
    /// read now, a private or internal repository's as `printed` says; no
    /// write credential is read (ADR-0068).
    ///
    /// # Errors
    ///
    /// Where no such rule plans re-runs, or what it plans from could not
    /// be read.
    pub async fn rerun_plans(&self, planning: PlansAsked<'_>) -> Result<RerunPlans, Stop> {
        self.through(Admission::Once, self.started(), Plans(planning))
            .await
    }

    /// Each job `chosen` names as `(job, head)` that the plans of `rule`
    /// hold now as `printed` prints them, re-run once by an operator signed in at its forge, shown
    /// how by `told`, waiting by `timer` (ADR-0068).
    ///
    /// # Errors
    ///
    /// Where no job is named, the store keeps no record of a re-run, or the
    /// plans could not be read.
    pub async fn rerun_apply<T: Timer>(
        &self,
        (planning, chosen): (PlansAsked<'_>, &[(String, String)]),
        timer: &T,
        told: &dyn Fn(SigningIn<'_>),
    ) -> Result<RerunApplied, Stop> {
        let outbound = self.config.http.clone().timed(self.host.var(TIMEOUT_MS));
        let connect = |base: Base<'_>| {
            self.host
                .connect(base, &outbound)
                .map(|http| self.counted(http))
        };
        let secrets = self.secrets();
        let applying = Applying {
            secrets: &secrets,
            connect: &connect,
            timer,
            told,
        };
        let apply = Apply::<H, T> {
            applying: &applying,
            planning,
            chosen,
        };
        self.through(Admission::Once, self.started(), apply).await
    }
}
