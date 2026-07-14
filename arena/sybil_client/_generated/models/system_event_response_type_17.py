from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_17_type import SystemEventResponseType17Type
from typing import cast






T = TypeVar("T", bound="SystemEventResponseType17")



@_attrs_define
class SystemEventResponseType17:
    """ 
        Attributes:
            activated_at_height (int):
            generation (int):
            market_ids (list[int]):
            policy_digest_hex (str):
            type_ (SystemEventResponseType17Type):
     """

    activated_at_height: int
    generation: int
    market_ids: list[int]
    policy_digest_hex: str
    type_: SystemEventResponseType17Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        activated_at_height = self.activated_at_height

        generation = self.generation

        market_ids = self.market_ids



        policy_digest_hex = self.policy_digest_hex

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "activated_at_height": activated_at_height,
            "generation": generation,
            "market_ids": market_ids,
            "policy_digest_hex": policy_digest_hex,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        activated_at_height = d.pop("activated_at_height")

        generation = d.pop("generation")

        market_ids = cast(list[int], d.pop("market_ids"))


        policy_digest_hex = d.pop("policy_digest_hex")

        type_ = SystemEventResponseType17Type(d.pop("type"))




        system_event_response_type_17 = cls(
            activated_at_height=activated_at_height,
            generation=generation,
            market_ids=market_ids,
            policy_digest_hex=policy_digest_hex,
            type_=type_,
        )


        system_event_response_type_17.additional_properties = d
        return system_event_response_type_17

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
