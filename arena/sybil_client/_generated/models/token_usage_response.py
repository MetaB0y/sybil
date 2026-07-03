from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="TokenUsageResponse")



@_attrs_define
class TokenUsageResponse:
    """ 
        Attributes:
            calls (int):
            completion_tokens (int):
            prompt_tokens (int):
            trader_name (str):
            avg_latency_s (float | None | Unset):
            latest_model (None | str | Unset):
     """

    calls: int
    completion_tokens: int
    prompt_tokens: int
    trader_name: str
    avg_latency_s: float | None | Unset = UNSET
    latest_model: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        calls = self.calls

        completion_tokens = self.completion_tokens

        prompt_tokens = self.prompt_tokens

        trader_name = self.trader_name

        avg_latency_s: float | None | Unset
        if isinstance(self.avg_latency_s, Unset):
            avg_latency_s = UNSET
        else:
            avg_latency_s = self.avg_latency_s

        latest_model: None | str | Unset
        if isinstance(self.latest_model, Unset):
            latest_model = UNSET
        else:
            latest_model = self.latest_model


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "calls": calls,
            "completion_tokens": completion_tokens,
            "prompt_tokens": prompt_tokens,
            "trader_name": trader_name,
        })
        if avg_latency_s is not UNSET:
            field_dict["avg_latency_s"] = avg_latency_s
        if latest_model is not UNSET:
            field_dict["latest_model"] = latest_model

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        calls = d.pop("calls")

        completion_tokens = d.pop("completion_tokens")

        prompt_tokens = d.pop("prompt_tokens")

        trader_name = d.pop("trader_name")

        def _parse_avg_latency_s(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_latency_s = _parse_avg_latency_s(d.pop("avg_latency_s", UNSET))


        def _parse_latest_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        latest_model = _parse_latest_model(d.pop("latest_model", UNSET))


        token_usage_response = cls(
            calls=calls,
            completion_tokens=completion_tokens,
            prompt_tokens=prompt_tokens,
            trader_name=trader_name,
            avg_latency_s=avg_latency_s,
            latest_model=latest_model,
        )


        token_usage_response.additional_properties = d
        return token_usage_response

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
