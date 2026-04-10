use crate::order::{PyOrderRequestCancel, PyOrderRequestOpen};
use crate::state::PyEngineStateSnapshot;
use barter::{
    engine::{
        Engine,
        state::{
            EngineState,
            global::DefaultGlobalData,
            instrument::{
                data::DefaultInstrumentMarketData,
                filter::InstrumentFilter,
            },
        },
    },
    strategy::{
        algo::AlgoStrategy,
        close_positions::ClosePositionsStrategy,
        on_disconnect::OnDisconnectStrategy,
        on_trading_disabled::OnTradingDisabled,
    },
};
use barter_execution::order::request::{OrderRequestCancel, OrderRequestOpen};
use barter_instrument::{
    asset::AssetIndex,
    exchange::{ExchangeId, ExchangeIndex},
    instrument::InstrumentIndex,
};
use pyo3::prelude::*;

/// The concrete EngineState type used throughout Python bindings.
pub type PyEngineState = EngineState<DefaultGlobalData, DefaultInstrumentMarketData>;

/// A strategy that delegates `generate_algo_orders` to a Python callable.
///
/// The Python callable receives a `PyEngineStateSnapshot` and must return a
/// tuple of `(List[OrderRequestCancel], List[OrderRequestOpen])`.
///
/// If no callback is provided, the strategy is a noop (no orders generated).
#[derive(Debug)]
pub struct PyStrategy {
    callback: Option<PyObject>,
}

impl PyStrategy {
    pub fn new(callback: Option<PyObject>) -> Self {
        Self { callback }
    }

    pub fn default_noop() -> Self {
        Self { callback: None }
    }
}

impl AlgoStrategy for PyStrategy {
    type State = PyEngineState;

    fn generate_algo_orders(
        &self,
        state: &Self::State,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel<ExchangeIndex, InstrumentIndex>>,
        impl IntoIterator<Item = OrderRequestOpen<ExchangeIndex, InstrumentIndex>>,
    ) {
        let Some(ref cb) = self.callback else {
            return (Vec::new(), Vec::new());
        };

        // Build a snapshot of the engine state for Python
        let snapshot = PyEngineStateSnapshot::from_state(state);

        // Acquire the GIL and call the Python strategy function
        let result: (Vec<OrderRequestCancel<ExchangeIndex, InstrumentIndex>>,
                      Vec<OrderRequestOpen<ExchangeIndex, InstrumentIndex>>) =
            Python::with_gil(|py| {
                let py_result = match cb.call1(py, (snapshot,)) {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!("Python strategy callback error: {e}");
                        return (Vec::new(), Vec::new());
                    }
                };

                // Parse the returned tuple: (cancels, opens)
                let tuple = match py_result.extract::<(Vec<PyOrderRequestCancel>, Vec<PyOrderRequestOpen>)>(py) {
                    Ok(t) => t,
                    Err(_) => {
                        // Try extracting just opens (common case: user returns only opens)
                        match py_result.extract::<Vec<PyOrderRequestOpen>>(py) {
                            Ok(opens) => (Vec::new(), opens),
                            Err(e) => {
                                eprintln!(
                                    "Python strategy must return (List[OrderRequestCancel], List[OrderRequestOpen]) \
                                     or List[OrderRequestOpen]: {e}"
                                );
                                return (Vec::new(), Vec::new());
                            }
                        }
                    }
                };

                let cancels = tuple.0.iter().map(|c| c.to_rust()).collect();
                let opens = tuple.1.iter().map(|o| o.to_rust()).collect();
                (cancels, opens)
            });

        result
    }
}

impl ClosePositionsStrategy for PyStrategy {
    type State = PyEngineState;

    fn close_positions_requests<'a>(
        &'a self,
        _state: &'a Self::State,
        _filter: &'a InstrumentFilter,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel> + 'a,
        impl IntoIterator<Item = OrderRequestOpen> + 'a,
    )
    where
        ExchangeIndex: 'a,
        AssetIndex: 'a,
        InstrumentIndex: 'a,
    {
        (std::iter::empty(), std::iter::empty())
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnDisconnectStrategy<Clock, State, ExecutionTxs, Risk>
    for PyStrategy
{
    type OnDisconnect = ();

    fn on_disconnect(
        _: &mut Engine<Clock, State, ExecutionTxs, Self, Risk>,
        _: ExchangeId,
    ) -> Self::OnDisconnect {
    }
}

impl<Clock, State, ExecutionTxs, Risk> OnTradingDisabled<Clock, State, ExecutionTxs, Risk>
    for PyStrategy
{
    type OnTradingDisabled = ();

    fn on_trading_disabled(
        _: &mut Engine<Clock, State, ExecutionTxs, Self, Risk>,
    ) -> Self::OnTradingDisabled {
    }
}
