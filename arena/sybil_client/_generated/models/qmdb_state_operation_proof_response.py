from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.qmdb_state_range_proof_response import QmdbStateRangeProofResponse





T = TypeVar("T", bound="QmdbStateOperationProofResponse")



@_attrs_define
class QmdbStateOperationProofResponse:
    """ 
        Attributes:
            activity_chunk_hex (str):
            location (int):
            range_ (QmdbStateRangeProofResponse):
     """

    activity_chunk_hex: str
    location: int
    range_: QmdbStateRangeProofResponse
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.qmdb_state_range_proof_response import QmdbStateRangeProofResponse
        activity_chunk_hex = self.activity_chunk_hex

        location = self.location

        range_ = self.range_.to_dict()


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "activity_chunk_hex": activity_chunk_hex,
            "location": location,
            "range": range_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.qmdb_state_range_proof_response import QmdbStateRangeProofResponse
        d = dict(src_dict)
        activity_chunk_hex = d.pop("activity_chunk_hex")

        location = d.pop("location")

        range_ = QmdbStateRangeProofResponse.from_dict(d.pop("range"))




        qmdb_state_operation_proof_response = cls(
            activity_chunk_hex=activity_chunk_hex,
            location=location,
            range_=range_,
        )


        qmdb_state_operation_proof_response.additional_properties = d
        return qmdb_state_operation_proof_response

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
