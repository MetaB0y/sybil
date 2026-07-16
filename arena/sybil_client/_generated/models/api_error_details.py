from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ApiErrorDetails")



@_attrs_define
class ApiErrorDetails:
    """ Stable machine-readable context attached to an API error.

        Attributes:
            market_id (int | None | Unset):
            market_status (None | str | Unset):
            message (None | str | Unset):
     """

    market_id: int | None | Unset = UNSET
    market_status: None | str | Unset = UNSET
    message: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id: int | None | Unset
        if isinstance(self.market_id, Unset):
            market_id = UNSET
        else:
            market_id = self.market_id

        market_status: None | str | Unset
        if isinstance(self.market_status, Unset):
            market_status = UNSET
        else:
            market_status = self.market_status

        message: None | str | Unset
        if isinstance(self.message, Unset):
            message = UNSET
        else:
            message = self.message


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if market_id is not UNSET:
            field_dict["market_id"] = market_id
        if market_status is not UNSET:
            field_dict["market_status"] = market_status
        if message is not UNSET:
            field_dict["message"] = message

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        def _parse_market_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        market_id = _parse_market_id(d.pop("market_id", UNSET))


        def _parse_market_status(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        market_status = _parse_market_status(d.pop("market_status", UNSET))


        def _parse_message(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        message = _parse_message(d.pop("message", UNSET))


        api_error_details = cls(
            market_id=market_id,
            market_status=market_status,
            message=message,
        )


        api_error_details.additional_properties = d
        return api_error_details

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
