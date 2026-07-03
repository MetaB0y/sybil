from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse





T = TypeVar("T", bound="QmdbStateExclusionProofResponse")



@_attrs_define
class QmdbStateExclusionProofResponse:
    """ 
        Attributes:
            operation (QmdbStateOperationProofResponse):
            variant (str):
            metadata_hex (None | str | Unset):
            span_key_hex (None | str | Unset):
            span_next_key_hex (None | str | Unset):
            span_value_hex (None | str | Unset):
     """

    operation: QmdbStateOperationProofResponse
    variant: str
    metadata_hex: None | str | Unset = UNSET
    span_key_hex: None | str | Unset = UNSET
    span_next_key_hex: None | str | Unset = UNSET
    span_value_hex: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse
        operation = self.operation.to_dict()

        variant = self.variant

        metadata_hex: None | str | Unset
        if isinstance(self.metadata_hex, Unset):
            metadata_hex = UNSET
        else:
            metadata_hex = self.metadata_hex

        span_key_hex: None | str | Unset
        if isinstance(self.span_key_hex, Unset):
            span_key_hex = UNSET
        else:
            span_key_hex = self.span_key_hex

        span_next_key_hex: None | str | Unset
        if isinstance(self.span_next_key_hex, Unset):
            span_next_key_hex = UNSET
        else:
            span_next_key_hex = self.span_next_key_hex

        span_value_hex: None | str | Unset
        if isinstance(self.span_value_hex, Unset):
            span_value_hex = UNSET
        else:
            span_value_hex = self.span_value_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "operation": operation,
            "variant": variant,
        })
        if metadata_hex is not UNSET:
            field_dict["metadata_hex"] = metadata_hex
        if span_key_hex is not UNSET:
            field_dict["span_key_hex"] = span_key_hex
        if span_next_key_hex is not UNSET:
            field_dict["span_next_key_hex"] = span_next_key_hex
        if span_value_hex is not UNSET:
            field_dict["span_value_hex"] = span_value_hex

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse
        d = dict(src_dict)
        operation = QmdbStateOperationProofResponse.from_dict(d.pop("operation"))




        variant = d.pop("variant")

        def _parse_metadata_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        metadata_hex = _parse_metadata_hex(d.pop("metadata_hex", UNSET))


        def _parse_span_key_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        span_key_hex = _parse_span_key_hex(d.pop("span_key_hex", UNSET))


        def _parse_span_next_key_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        span_next_key_hex = _parse_span_next_key_hex(d.pop("span_next_key_hex", UNSET))


        def _parse_span_value_hex(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        span_value_hex = _parse_span_value_hex(d.pop("span_value_hex", UNSET))


        qmdb_state_exclusion_proof_response = cls(
            operation=operation,
            variant=variant,
            metadata_hex=metadata_hex,
            span_key_hex=span_key_hex,
            span_next_key_hex=span_next_key_hex,
            span_value_hex=span_value_hex,
        )


        qmdb_state_exclusion_proof_response.additional_properties = d
        return qmdb_state_exclusion_proof_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
