from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="ProofJobAckResponse")



@_attrs_define
class ProofJobAckResponse:
    """ 
        Attributes:
            acknowledged (bool):
            height (int):
            transport_digest (str):
     """

    acknowledged: bool
    height: int
    transport_digest: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        acknowledged = self.acknowledged

        height = self.height

        transport_digest = self.transport_digest


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "acknowledged": acknowledged,
            "height": height,
            "transport_digest": transport_digest,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        acknowledged = d.pop("acknowledged")

        height = d.pop("height")

        transport_digest = d.pop("transport_digest")

        proof_job_ack_response = cls(
            acknowledged=acknowledged,
            height=height,
            transport_digest=transport_digest,
        )


        proof_job_ack_response.additional_properties = d
        return proof_job_ack_response

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
