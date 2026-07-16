from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.api_error_details import ApiErrorDetails





T = TypeVar("T", bound="ApiErrorResponse")



@_attrs_define
class ApiErrorResponse:
    """ Error envelope returned by every non-success REST response.

        Attributes:
            code (str):
            error (str):
            details (ApiErrorDetails | None | Unset):
     """

    code: str
    error: str
    details: ApiErrorDetails | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.api_error_details import ApiErrorDetails
        code = self.code

        error = self.error

        details: dict[str, Any] | None | Unset
        if isinstance(self.details, Unset):
            details = UNSET
        elif isinstance(self.details, ApiErrorDetails):
            details = self.details.to_dict()
        else:
            details = self.details


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "code": code,
            "error": error,
        })
        if details is not UNSET:
            field_dict["details"] = details

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.api_error_details import ApiErrorDetails
        d = dict(src_dict)
        code = d.pop("code")

        error = d.pop("error")

        def _parse_details(data: object) -> ApiErrorDetails | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                details_type_1 = ApiErrorDetails.from_dict(data)



                return details_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ApiErrorDetails | None | Unset, data)

        details = _parse_details(d.pop("details", UNSET))


        api_error_response = cls(
            code=code,
            error=error,
            details=details,
        )


        api_error_response.additional_properties = d
        return api_error_response

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
