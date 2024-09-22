use anyhow::Result;

pub(crate) fn clone_string_attribute<R: gimli::Reader>(
    dwarf: &gimli::Dwarf<R>,
    unit: &gimli::Unit<R, R::Offset>,
    attr: gimli::AttributeValue<R>,
) -> Result<String> {
    Ok(dwarf
        .attr_string(unit, attr)?
        .to_string()?
        .as_ref()
        .to_string())
}

pub(crate) fn unit_ref_offset_to_absolute_offset<R: gimli::Reader>(
    offset: gimli::UnitOffset<R::Offset>,
    unit: &gimli::Unit<R>,
) -> R::Offset {
    match unit.offset {
        gimli::UnitSectionOffset::DebugInfoOffset(v) => v.0 + offset.0,
        gimli::UnitSectionOffset::DebugTypesOffset(v) => v.0 + offset.0,
    }
}
