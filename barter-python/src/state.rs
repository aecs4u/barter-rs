use crate::decimal::decimal_to_f64;
use crate::strategy::PyEngineState;
use barter::engine::state::instrument::data::InstrumentDataState;
use barter_instrument::Side;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

/// A snapshot of the engine state, copied into Python-friendly types.
///
/// This avoids holding Rust borrows across the FFI boundary. The snapshot is
/// created once per engine tick before calling the Python strategy callback.
#[pyclass(name = "EngineState", module = "barter")]
#[derive(Debug, Clone)]
pub struct PyEngineStateSnapshot {
    /// List of instrument snapshots, indexed by InstrumentIndex.
    pub instruments: Vec<InstrumentSnapshot>,
    /// List of asset balance snapshots.
    pub balances: Vec<BalanceSnapshot>,
}

#[derive(Debug, Clone)]
pub struct InstrumentSnapshot {
    pub index: usize,
    pub name_internal: String,
    pub name_exchange: String,
    pub kind: String,
    pub price: Option<f64>,
    pub position_side: Option<String>,
    pub position_quantity: Option<f64>,
    pub position_entry_price: Option<f64>,
    pub position_pnl_unrealised: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct BalanceSnapshot {
    pub asset_name: String,
    pub total: f64,
    pub free: f64,
}

impl PyEngineStateSnapshot {
    /// Build a snapshot from the live engine state.
    pub fn from_state(state: &PyEngineState) -> Self {
        let mut instruments = Vec::new();
        for (_name, inst_state) in state.instruments.0.iter() {
            let price = inst_state.data.price().map(|d| decimal_to_f64(&d));

            let (pos_side, pos_qty, pos_entry, pos_pnl) = match &inst_state.position.current {
                Some(pos) => {
                    let side = match pos.side {
                        Side::Buy => "buy",
                        Side::Sell => "sell",
                    };
                    (
                        Some(side.to_string()),
                        Some(decimal_to_f64(&pos.quantity_abs)),
                        Some(decimal_to_f64(&pos.price_entry_average)),
                        Some(decimal_to_f64(&pos.pnl_unrealised)),
                    )
                }
                None => (None, None, None, None),
            };

            let kind = match &inst_state.instrument.kind {
                barter_instrument::instrument::kind::InstrumentKind::Spot => "spot",
                barter_instrument::instrument::kind::InstrumentKind::Perpetual(_) => "perpetual",
                barter_instrument::instrument::kind::InstrumentKind::Future(_) => "future",
                barter_instrument::instrument::kind::InstrumentKind::Option(_) => "option",
            };

            instruments.push(InstrumentSnapshot {
                index: inst_state.key.0,
                name_internal: inst_state.instrument.name_internal.to_string(),
                name_exchange: inst_state.instrument.name_exchange.to_string(),
                kind: kind.to_string(),
                price,
                position_side: pos_side,
                position_quantity: pos_qty,
                position_entry_price: pos_entry,
                position_pnl_unrealised: pos_pnl,
            });
        }

        let mut balances = Vec::new();
        for (_key, asset_state) in state.assets.0.iter() {
            let (total, free) = match &asset_state.balance {
                Some(timed) => (
                    decimal_to_f64(&timed.value.total),
                    decimal_to_f64(&timed.value.free),
                ),
                None => (0.0, 0.0),
            };
            balances.push(BalanceSnapshot {
                asset_name: asset_state.asset.name_internal.to_string(),
                total,
                free,
            });
        }

        Self {
            instruments,
            balances,
        }
    }
}

#[pymethods]
impl PyEngineStateSnapshot {
    /// Get the number of instruments.
    fn __len__(&self) -> usize {
        self.instruments.len()
    }

    /// Get instrument data as a list of dicts.
    fn instruments(&self, py: Python<'_>) -> PyResult<PyObject> {
        let list = PyList::empty(py);
        for inst in &self.instruments {
            let d = PyDict::new(py);
            d.set_item("index", inst.index)?;
            d.set_item("name_internal", &inst.name_internal)?;
            d.set_item("name_exchange", &inst.name_exchange)?;
            d.set_item("kind", &inst.kind)?;
            d.set_item("price", inst.price)?;
            d.set_item("position_side", inst.position_side.as_deref())?;
            d.set_item("position_quantity", inst.position_quantity)?;
            d.set_item("position_entry_price", inst.position_entry_price)?;
            d.set_item("position_pnl_unrealised", inst.position_pnl_unrealised)?;
            list.append(d)?;
        }
        Ok(list.into())
    }

    /// Get balances as a list of dicts.
    fn balances(&self, py: Python<'_>) -> PyResult<PyObject> {
        let list = PyList::empty(py);
        for bal in &self.balances {
            let d = PyDict::new(py);
            d.set_item("asset", &bal.asset_name)?;
            d.set_item("total", bal.total)?;
            d.set_item("free", bal.free)?;
            list.append(d)?;
        }
        Ok(list.into())
    }

    /// Get the current price for an instrument by index.
    fn price(&self, instrument_index: usize) -> Option<f64> {
        self.instruments
            .iter()
            .find(|i| i.index == instrument_index)
            .and_then(|i| i.price)
    }

    /// Get the current position for an instrument by index (or None if flat).
    fn position(&self, instrument_index: usize, py: Python<'_>) -> PyResult<PyObject> {
        let inst = self
            .instruments
            .iter()
            .find(|i| i.index == instrument_index);

        match inst {
            Some(i) if i.position_side.is_some() => {
                let d = PyDict::new(py);
                d.set_item("side", i.position_side.as_deref())?;
                d.set_item("quantity", i.position_quantity)?;
                d.set_item("entry_price", i.position_entry_price)?;
                d.set_item("pnl_unrealised", i.position_pnl_unrealised)?;
                Ok(d.into())
            }
            _ => Ok(py.None()),
        }
    }

    /// Get the balance for a named asset (or None).
    fn balance(&self, asset: &str, py: Python<'_>) -> PyResult<PyObject> {
        match self.balances.iter().find(|b| b.asset_name == asset) {
            Some(bal) => {
                let d = PyDict::new(py);
                d.set_item("asset", &bal.asset_name)?;
                d.set_item("total", bal.total)?;
                d.set_item("free", bal.free)?;
                Ok(d.into())
            }
            None => Ok(py.None()),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "EngineState(instruments={}, balances={})",
            self.instruments.len(),
            self.balances.len(),
        )
    }
}

pub fn register(parent: &Bound<'_, PyModule>) -> PyResult<()> {
    parent.add_class::<PyEngineStateSnapshot>()?;
    Ok(())
}
