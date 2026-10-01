//! Validate before exposing the upstream infallible iterators.
//!
//! The public wrappers own immutable protobuf messages. Checking every delta,
//! multiplication, parallel array and string index once at construction therefore
//! makes all subsequent traversals safe, including IndexedReader's extra passes.

use crate::error::{Error, Result};
use crate::proto::osmformat::{DenseInfo, DenseNodes, Info, PrimitiveBlock};

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn ensure(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn add(left: i64, right: i64, field: &'static str) -> Result<i64> {
    left.checked_add(right)
        .ok_or_else(|| invalid(format!("overflow in {field} delta")))
}

fn scaled(value: i64, scale: i32, offset: i64, field: &'static str) -> Result<()> {
    value
        .checked_mul(i64::from(scale))
        .and_then(|value| value.checked_add(offset))
        .ok_or_else(|| invalid(format!("overflow in {field} scale/offset")))?;
    Ok(())
}

fn index(value: i64, count: usize) -> Result<()> {
    ensure(
        usize::try_from(value).is_ok_and(|value| value < count),
        "stringtable index out of bounds",
    )
}

fn tags(keys: &[u32], values: &[u32], count: usize) -> Result<()> {
    ensure(keys.len() == values.len(), "unequal tag key/value arrays")?;
    for &value in keys.iter().chain(values) {
        index(i64::from(value), count)?;
    }
    Ok(())
}

fn info(info: Option<&Info>, block: &PrimitiveBlock) -> Result<()> {
    if let Some(info) = info {
        if info.has_user_sid() {
            index(i64::from(info.user_sid()), block.stringtable.s.len())?;
        }
        if info.has_timestamp() {
            scaled(info.timestamp(), block.date_granularity(), 0, "timestamp")?;
        }
    }
    Ok(())
}

fn deltas(values: &[i64], field: &'static str) -> Result<()> {
    let mut value = 0;
    for &delta in values {
        value = add(value, delta, field)?;
    }
    Ok(())
}

fn coordinates(values: &[i64], scale: i32, offset: i64, field: &'static str) -> Result<()> {
    let mut value = 0;
    for &delta in values {
        value = add(value, delta, field)?;
        scaled(value, scale, offset, field)?;
    }
    Ok(())
}

fn dense_info(info: &DenseInfo, block: &PrimitiveBlock, count: usize) -> Result<()> {
    for length in [
        info.version.len(),
        info.timestamp.len(),
        info.changeset.len(),
        info.uid.len(),
        info.user_sid.len(),
        info.visible.len(),
    ] {
        ensure(
            length == 0 || length == count,
            "unequal dense metadata arrays",
        )?;
    }
    coordinates(
        &info.timestamp,
        block.date_granularity(),
        0,
        "dense timestamp",
    )?;
    deltas(&info.changeset, "dense changeset")?;
    for (values, field) in [(&info.uid, "dense uid"), (&info.user_sid, "dense user_sid")] {
        let mut value = 0_i32;
        for &delta in values {
            value = value
                .checked_add(delta)
                .ok_or_else(|| invalid(format!("overflow in {field} delta")))?;
            if field == "dense user_sid" {
                index(i64::from(value), block.stringtable.s.len())?;
            }
        }
    }
    Ok(())
}

fn dense(nodes: &DenseNodes, block: &PrimitiveBlock) -> Result<()> {
    let count = nodes.id.len();
    ensure(
        nodes.lat.len() == count && nodes.lon.len() == count,
        "unequal dense node id/coordinate arrays",
    )?;
    deltas(&nodes.id, "dense id")?;
    coordinates(
        &nodes.lat,
        block.granularity(),
        block.lat_offset(),
        "dense latitude",
    )?;
    coordinates(
        &nodes.lon,
        block.granularity(),
        block.lon_offset(),
        "dense longitude",
    )?;
    if let Some(info) = nodes.denseinfo.as_ref() {
        dense_info(info, block, count)?;
    }
    // Empty keys_vals means every node is tagless. Otherwise each node requires
    // its own delimiter, even if its tag list is empty.
    if !nodes.keys_vals.is_empty() {
        let mut tags = nodes.keys_vals.iter();
        for _ in 0..count {
            loop {
                let key = tags
                    .next()
                    .ok_or_else(|| invalid("missing dense tag delimiter"))?;
                if *key == 0 {
                    break;
                }
                let value = tags
                    .next()
                    .ok_or_else(|| invalid("incomplete dense tag pair"))?;
                index(i64::from(*key), block.stringtable.s.len())?;
                index(i64::from(*value), block.stringtable.s.len())?;
            }
        }
        ensure(tags.next().is_none(), "extra dense tags after final node")?;
    }
    Ok(())
}

pub(crate) fn primitive_block(block: &PrimitiveBlock) -> Result<()> {
    ensure(
        block.granularity() > 0,
        "non-positive coordinate granularity",
    )?;
    ensure(
        block.date_granularity() > 0,
        "non-positive date granularity",
    )?;
    ensure(
        block.stringtable.s.first().is_some_and(Vec::is_empty),
        "stringtable must begin with an empty string",
    )?;
    for (index, string) in block.stringtable.s.iter().enumerate() {
        std::str::from_utf8(string)
            .map_err(|_| invalid(format!("invalid UTF-8 at stringtable index {index}")))?;
    }
    let count = block.stringtable.s.len();
    for group in &block.primitivegroup {
        for node in &group.nodes {
            tags(&node.keys, &node.vals, count)?;
            info(node.info.as_ref(), block)?;
            scaled(
                node.lat(),
                block.granularity(),
                block.lat_offset(),
                "node latitude",
            )?;
            scaled(
                node.lon(),
                block.granularity(),
                block.lon_offset(),
                "node longitude",
            )?;
        }
        if let Some(nodes) = group.dense.as_ref() {
            dense(nodes, block)?;
        }
        for way in &group.ways {
            tags(&way.keys, &way.vals, count)?;
            info(way.info.as_ref(), block)?;
            deltas(&way.refs, "way reference")?;
            if !way.lat.is_empty() || !way.lon.is_empty() {
                ensure(
                    way.lat.len() == way.refs.len() && way.lon.len() == way.refs.len(),
                    "unequal LocationsOnWays arrays",
                )?;
                coordinates(
                    &way.lat,
                    block.granularity(),
                    block.lat_offset(),
                    "way latitude",
                )?;
                coordinates(
                    &way.lon,
                    block.granularity(),
                    block.lon_offset(),
                    "way longitude",
                )?;
            }
        }
        for relation in &group.relations {
            tags(&relation.keys, &relation.vals, count)?;
            info(relation.info.as_ref(), block)?;
            ensure(
                relation.memids.len() == relation.roles_sid.len()
                    && relation.memids.len() == relation.types.len(),
                "unequal relation member arrays",
            )?;
            deltas(&relation.memids, "relation member")?;
            for &role in &relation.roles_sid {
                index(i64::from(role), count)?;
            }
            for member_type in &relation.types {
                ensure(
                    member_type.enum_value().is_ok(),
                    "unknown relation member type",
                )?;
            }
        }
    }
    Ok(())
}
