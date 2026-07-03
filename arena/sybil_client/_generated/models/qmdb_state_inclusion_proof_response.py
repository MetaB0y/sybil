from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse





T = TypeVar("T", bound="QmdbStateInclusionProofResponse")



@_attrs_define
class QmdbStateInclusionProofResponse:
    """ 
        Attributes:
            next_key_hex (str):
            operation (QmdbStateOperationProofResponse):
     """

    next_key_hex: str
    operation: QmdbStateOperationProofResponse
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse
        next_key_hex = self.next_key_hex

        operation = self.operation.to_dict()


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "next_key_hex": next_key_hex,
            "operation": operation,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.qmdb_state_operation_proof_response import QmdbStateOperationProofResponse
        d = dict(src_dict)
        next_key_hex = d.pop("next_key_hex")

        operation = QmdbStateOperationProofResponse.from_dict(d.pop("operation"))




        qmdb_state_inclusion_proof_response = cls(
            next_key_hex=next_key_hex,
            operation=operation,
        )


        qmdb_state_inclusion_proof_response.additional_properties = d
        return qmdb_state_inclusion_proof_response

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
