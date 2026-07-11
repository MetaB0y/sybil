from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.account_response import AccountResponse
from ...models.set_profile_request import SetProfileRequest
from typing import cast



def _get_kwargs(
    id: int,
    *,
    body: SetProfileRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/accounts/{id}/profile".format(id=quote(str(id), safe=""),),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> AccountResponse | Any | None:
    if response.status_code == 200:
        response_200 = AccountResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 409:
        response_409 = cast(Any, None)
        return response_409

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[AccountResponse | Any]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SetProfileRequest,

) -> Response[AccountResponse | Any]:
    """ POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)

    Args:
        id (int):
        body (SetProfileRequest): Common P256/WebAuthn signature envelope shared by SYB-60
            account-management
            mutations. Mirrors the fields on `CreateSignedBridgeWithdrawalRequest`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountResponse | Any]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SetProfileRequest,

) -> AccountResponse | Any | None:
    """ POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)

    Args:
        id (int):
        body (SetProfileRequest): Common P256/WebAuthn signature envelope shared by SYB-60
            account-management
            mutations. Mirrors the fields on `CreateSignedBridgeWithdrawalRequest`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountResponse | Any
     """


    return sync_detailed(
        id=id,
client=client,
body=body,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SetProfileRequest,

) -> Response[AccountResponse | Any]:
    """ POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)

    Args:
        id (int):
        body (SetProfileRequest): Common P256/WebAuthn signature envelope shared by SYB-60
            account-management
            mutations. Mirrors the fields on `CreateSignedBridgeWithdrawalRequest`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountResponse | Any]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SetProfileRequest,

) -> AccountResponse | Any | None:
    """ POST /v1/accounts/{id}/profile — set/clear opt-in profile (signed) (SYB-60)

    Args:
        id (int):
        body (SetProfileRequest): Common P256/WebAuthn signature envelope shared by SYB-60
            account-management
            mutations. Mirrors the fields on `CreateSignedBridgeWithdrawalRequest`.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountResponse | Any
     """


    return (await asyncio_detailed(
        id=id,
client=client,
body=body,

    )).parsed
