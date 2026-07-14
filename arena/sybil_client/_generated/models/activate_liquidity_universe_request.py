from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast






T = TypeVar("T", bound="ActivateLiquidityUniverseRequest")



@_attrs_define
class ActivateLiquidityUniverseRequest:
    """ 
        Attributes:
            generation (int):
            market_ids (list[int]):
            policy_digest_hex (str):
     """

    generation: int
    market_ids: list[int]
    policy_digest_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        generation = self.generation

        market_ids = self.market_ids



        policy_digest_hex = self.policy_digest_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "generation": generation,
            "market_ids": market_ids,
            "policy_digest_hex": policy_digest_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        generation = d.pop("generation")

        market_ids = cast(list[int], d.pop("market_ids"))


        policy_digest_hex = d.pop("policy_digest_hex")

        activate_liquidity_universe_request = cls(
            generation=generation,
            market_ids=market_ids,
            policy_digest_hex=policy_digest_hex,
        )


        activate_liquidity_universe_request.additional_properties = d
        return activate_liquidity_universe_request

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
